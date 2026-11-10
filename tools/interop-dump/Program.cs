using System.Text.Json;
using System.Text.Json.Serialization;
using Facepunch.InteropGen;

namespace Underbox.InteropDump;

/// <summary>
/// Runs Facepunch's InteropGen pipeline on one .def and writes the resolved native export table
/// (slot order, ABI types per slot), the managed callback table and the struct list as JSON.
/// Slot order comes from InteropGen's own NativeExportTable, so it matches the managed reader exactly.
/// </summary>
internal static class Program
{
	public static int Main( string[] args )
	{
		if ( args is ["generate", var engineDir] )
			return Facepunch.InteropGen.Program.ProcessManifest( engineDir, skipNative: true ) ? 0 : 1;

		if ( args is ["layouts", var abiJson, var engineDll, var layoutsOut] )
			return Layouts.Run( abiJson, engineDll, layoutsOut );

		if ( args is not ["dump", var def, var output] )
		{
			Console.Error.WriteLine( """
				usage: interop-dump dump <path/to/engine.def> <out.json>
				       interop-dump generate <sbox-public/engine>                     managed bindings only
				       interop-dump layouts <engine.json> <Sandbox.Engine.dll> <out.json>
				""" );
			return 2;
		}

		Definition d = InteropPipeline.Build( Path.GetFullPath( def ) );
		SkipPolicy skip = new( d );

		var dump = new AbiDump(
			d.Ident,
			d.NativeDll,
			d.Hash,
			d.Structs.Where( s => !skip.ShouldSkip( s ) ).Select( ToStruct ).ToList(),
			NativeExportTable.Slots( d, skip ).Select( ( s, i ) => ToSlot( s, i ) ).ToList(),
			d.ManagedClasses.Where( c => !skip.ShouldSkip( c ) )
				.SelectMany( c => c.Functions )
				.Select( ( f, i ) => ToCallback( f, i ) ).ToList()
		);

		var options = new JsonSerializerOptions
		{
			WriteIndented = true,
			PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
			DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
			Converters = { new JsonStringEnumConverter( JsonNamingPolicy.SnakeCaseLower ) },
		};
		File.WriteAllText( output, JsonSerializer.Serialize( dump, options ) );

		Console.WriteLine( $"{d.Ident}: hash {d.Hash}, {dump.Slots.Count} slots, {dump.Callbacks.Count} callbacks, {dump.Structs.Count} structs" );
		return 0;
	}

	static StructInfo ToStruct( Struct s ) => new(
		s.NativeNameWithNamespace,
		s.ManagedNameWithNamespace,
		s.IsEnum ? "enum" : s.IsPointer ? "pointer" : "struct",
		s.HasAttribute( "small" )
	);

	static SlotInfo ToSlot( NativeSlot s, int index )
	{
		switch ( s.Kind )
		{
			case NativeSlotKind.Error:
				return new SlotInfo( index, s.Kind, null, "__error", null, [new( "message", "IntPtr", "const char*", null, "String" )], Void, false );

			case NativeSlotKind.CastFromTo:
			case NativeSlotKind.CastToFrom:
			{
				var name = s.Kind == NativeSlotKind.CastFromTo
					? $"From_{s.BaseClass.ManagedName}_To_{s.Class.ManagedName}"
					: $"To_{s.BaseClass.ManagedName}_From_{s.Class.ManagedName}";
				var (from, to) = s.Kind == NativeSlotKind.CastFromTo ? (s.BaseClass, s.Class) : (s.Class, s.BaseClass);
				return new SlotInfo( index, s.Kind, ToClass( s.Class ), name, s.BaseClass.NativeNameWithNamespace,
					[new( "obj", "IntPtr", "void*", null, "DefinedClass", from.NativeNameWithNamespace )],
					new( "returnvalue", "IntPtr", "void*", null, "DefinedClass", to.NativeNameWithNamespace ), true );
			}

			case NativeSlotKind.Function:
			{
				Function f = s.Function;
				var native = s.Class.SelfArg( true, f.Static ).Concat( f.Parameters ).Where( a => a.IsRealArgument ).ToList();
				var managed = s.Class.SelfArg( false, f.Static ).Concat( f.Parameters ).Where( a => a.IsRealArgument ).ToList();
				var args = managed.Zip( native, ( m, n ) => Describe( new ArgInfo(
					m.Name, m.DelegateType( Side.Managed, Dir.Outgoing ), n.DelegateType( Side.Native, Dir.Incoming ), Flags( m ) ), n ) ).ToList();
				var ret = Describe( new ArgInfo( "returnvalue", f.Return.DelegateType( Side.Managed, Dir.Incoming ),
					f.Return.DelegateType( Side.Native, Dir.Outgoing ), Flags( f.Return ) ), f.Return );
				var declaredIn = f.Class != null && f.Class != s.Class ? f.Class.NativeNameWithNamespace : null;
				return new SlotInfo( index, s.Kind, ToClass( s.Class ), f.MangledName, declaredIn, args, ret, f.IsNoGC, f.Static, f.Name );
			}

			case NativeSlotKind.VariableGet:
			{
				Variable v = s.Variable;
				return new SlotInfo( index, s.Kind, ToClass( s.Class ), $"Get__{v.MangledName}", null,
					[new( "self", "IntPtr", "void*", null )],
					Describe( new( "returnvalue", v.Return.DelegateType( Side.Managed, Dir.Incoming ), v.Return.DelegateType( Side.Native, Dir.Outgoing ), null ), v.Return ),
					true, v.Static, v.Name );
			}

			case NativeSlotKind.VariableSet:
			{
				Variable v = s.Variable;
				return new SlotInfo( index, s.Kind, ToClass( s.Class ), $"Set__{v.MangledName}", null,
					[new( "self", "IntPtr", "void*", null ),
					 Describe( new( "value", v.Return.DelegateType( Side.Managed, Dir.Outgoing ), v.Return.DelegateType( Side.Native, Dir.Incoming ), null ), v.Return )],
					Void, true, v.Static, v.Name );
			}
		}

		throw new InvalidOperationException( $"unknown slot kind {s.Kind}" );
	}

	static CallbackInfo ToCallback( Function f, int index )
	{
		var c = f.Class;
		var args = c.SelfArg( true, f.Static ).Concat( f.Parameters ).Where( a => a.IsRealArgument )
			.Select( a => Describe( new ArgInfo( a.Name, a.DelegateType( Side.Managed, Dir.Incoming ), a.DelegateType( Side.Native, Dir.Outgoing ), Flags( a ) ), a ) ).ToList();
		var ret = Describe( new ArgInfo( "returnvalue", f.Return.DelegateType( Side.Managed, Dir.Incoming ), f.Return.DelegateType( Side.Native, Dir.Outgoing ), null ), f.Return );
		return new CallbackInfo( index, ToClass( c ), f.MangledName, f.Name, args, ret );
	}

	static ClassInfo ToClass( Class c ) => new(
		c.NativeNameWithNamespace,
		c.ManagedNameWithNamespace,
		c.Static,
		c.Accessor,
		c.IsHandleType ? c.HandleIndex : null,
		c.IsResourceHandle ? c.ResourceHandleName : null
	);

	static string[] Flags( Arg a ) => a.Flags is { Length: > 0 } f ? f : null;

	static string KindOf( Arg a ) => a.GetType().Name.StartsWith( "Arg" ) ? a.GetType().Name[3..] : a.GetType().Name;

	static ArgInfo Describe( ArgInfo info, Arg a ) => Unwrap( a ) switch
	{
		ArgDefinedClass c => info with { Kind = KindOf( c ), Of = c.Class.NativeNameWithNamespace },
		ArgArray arr => info with { Kind = KindOf( arr ), Of = KindOf( Unwrap( arr.Base ) ) },
		ArgDefinedStruct s => info with { Kind = KindOf( s ), Of = s.Type.ManagedNameWithNamespace },
		var u => info with { Kind = KindOf( u ) },
	};

	// Flag wrappers (out, ref, CastTo[...]) carry the flags; the wrapped Arg says what the value is.
	static Arg Unwrap( Arg a )
	{
		while ( a is ArgWrapper w && a is not ArgArray ) a = w.Base;
		return a;
	}

	static readonly ArgInfo Void = new( "returnvalue", "void", "void", null, "Void" );
}

internal record AbiDump( string Ident, string NativeDll, int Hash, List<StructInfo> Structs, List<SlotInfo> Slots, List<CallbackInfo> Callbacks );
internal record StructInfo( string Native, string Managed, string Kind, bool Small );
internal record ClassInfo( string Native, string Managed, bool Static, bool Accessor, string Handle, string ResourceHandle );
/// <param name="Kind">InteropGen's Arg class without the prefix (Int, String, DefinedClass, DefinedStruct, Array, ...).</param>
/// <param name="Of">DefinedClass: the class's native name. DefinedStruct: the struct's managed name. Array: the element's kind.</param>
internal record ArgInfo( string Name, string Managed, string Native, string[] Flags, string Kind = null, string Of = null );
/// <param name="BaseClass">Casts: the base class. Functions: the class that declares an inherited function, null if declared here.</param>
internal record SlotInfo( int Index, NativeSlotKind Kind, ClassInfo Class, string Name, string BaseClass, List<ArgInfo> Args, ArgInfo Ret, bool NoGc, bool Static = false, string Member = null );
internal record CallbackInfo( int Index, ClassInfo Class, string Name, string Member, List<ArgInfo> Args, ArgInfo Ret );
