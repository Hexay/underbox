using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Loader;
using System.Text.Json;

namespace Underbox.InteropDump;

/// <summary>
/// Reflects the built Sandbox.Engine.dll for the CLR's layout of every struct the interop table names
/// (and every struct those contain): size as the managed side's <c>sizeof</c>, which native checks
/// against at <c>igen_engine</c>, and each field's offset.
/// </summary>
internal static class Layouts
{
	public static int Run( string abiJson, string engineDll, string output )
	{
		var dir = Path.GetDirectoryName( Path.GetFullPath( engineDll ) )!;
		var alc = new AssemblyLoadContext( "engine", isCollectible: false );
		alc.Resolving += ( ctx, name ) =>
		{
			var p = Path.Combine( dir, name.Name + ".dll" );
			return File.Exists( p ) ? ctx.LoadFromAssemblyPath( p ) : null;
		};
		var engine = alc.LoadFromAssemblyPath( Path.GetFullPath( engineDll ) );

		using var doc = JsonDocument.Parse( File.ReadAllText( abiJson ) );
		// Every declared kind: native checks managed's sizeof for enums and pointer handles too.
		var wanted = doc.RootElement.GetProperty( "structs" ).EnumerateArray()
			.Select( s => s.GetProperty( "managed" ).GetString()! )
			.ToList();

		// Math types (Vector3, Transform, BBox...) live in referenced assemblies such as Sandbox.System.
		var assemblies = new[] { engine }.Concat( engine.GetReferencedAssemblies()
			.Where( n => File.Exists( Path.Combine( dir, n.Name + ".dll" ) ) )
			.Select( alc.LoadFromAssemblyName ) );
		var types = assemblies.SelectMany( Loadable ).Where( t => t.IsValueType ).ToList();
		var layouts = new Dictionary<string, object>();
		var missing = new List<string>();
		var queue = new Queue<Type>();
		var declared = new Dictionary<Type, List<string>>();

		foreach ( var name in wanted )
		{
			var t = Find( types, name );
			if ( t == null ) { missing.Add( name ); continue; }
			queue.Enqueue( t );
			if ( !declared.TryGetValue( t, out var names ) ) declared[t] = names = [];
			names.Add( name );
		}

		while ( queue.TryDequeue( out var t ) )
		{
			if ( layouts.ContainsKey( t.FullName! ) ) continue;
			var fields = new List<object>();
			foreach ( var f in t.IsEnum ? [] : t.GetFields( BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic ) )
			{
				var fixedBuf = f.GetCustomAttribute<FixedBufferAttribute>();
				var ft = fixedBuf?.ElementType ?? f.FieldType;
				if ( ft.IsValueType && !ft.IsPrimitive && !ft.IsEnum && ft != t ) queue.Enqueue( ft );
				fields.Add( new
				{
					name = f.Name,
					offset = OffsetOf( f ),
					type = TypeName( ft ),
					count = fixedBuf?.Length ?? 1,
				} );
			}
			layouts[t.FullName!] = new
			{
				managed = t.FullName!.Replace( '+', '.' ),
				underlying = t.IsEnum ? TypeName( Enum.GetUnderlyingType( t ) ) : null,
				declared = declared.GetValueOrDefault( t ) ?? [],
				size = SizeOf( t ),
				fields,
			};
		}

		File.WriteAllText( output, JsonSerializer.Serialize( layouts.Values, new JsonSerializerOptions { WriteIndented = true } ) );
		Console.WriteLine( $"{layouts.Count} layouts, {missing.Count} declared structs not found in {Path.GetFileName( engineDll )}" );
		foreach ( var m in missing ) Console.WriteLine( $"  missing: {m}" );
		return missing.Count == 0 ? 0 : 1;
	}

	static IEnumerable<Type> Loadable( Assembly a )
	{
		try { return a.GetTypes(); }
		catch ( ReflectionTypeLoadException e ) { return e.Types.Where( t => t != null )!; }
	}

	// Managed names in .def files are written with or without their namespace.
	static Type Find( List<Type> types, string name ) =>
		types.FirstOrDefault( t => t.FullName!.Replace( '+', '.' ) == name )
		?? types.SingleOrDefault( t => t.Name == name.Split( '.' ).Last() && t.Namespace is "Sandbox" or "NativeEngine" or null )
		?? types.FirstOrDefault( t => t.Name == name.Split( '.' ).Last() );

	static string TypeName( Type t )
	{
		if ( t.IsEnum ) return $"enum:{TypeName( Enum.GetUnderlyingType( t ) )}";
		if ( t.IsPointer || t == typeof( IntPtr ) || t == typeof( UIntPtr ) ) return "IntPtr";
		return t.FullName switch
		{
			"System.Boolean" => "bool", "System.Byte" => "byte", "System.SByte" => "sbyte",
			"System.Int16" => "short", "System.UInt16" => "ushort", "System.Int32" => "int",
			"System.UInt32" => "uint", "System.Int64" => "long", "System.UInt64" => "ulong",
			"System.Single" => "float", "System.Double" => "double", "System.Char" => "char",
			_ => t.FullName!.Replace( '+', '.' ),
		};
	}

	// The runtime's own layout, as a function pointer sees the struct: Marshal.OffsetOf gives the
	// marshalled layout instead, which differs wherever a bool (4-byte BOOL when marshalled) is.
	static int OffsetOf( FieldInfo f )
	{
		var dm = new System.Reflection.Emit.DynamicMethod( "offset", typeof( long ), [typeof( IntPtr )], typeof( Layouts ).Module, skipVisibility: true );
		var il = dm.GetILGenerator();
		il.Emit( System.Reflection.Emit.OpCodes.Ldarg_0 );
		il.Emit( System.Reflection.Emit.OpCodes.Ldflda, f );
		il.Emit( System.Reflection.Emit.OpCodes.Ldarg_0 );
		il.Emit( System.Reflection.Emit.OpCodes.Sub );
		il.Emit( System.Reflection.Emit.OpCodes.Conv_I8 );
		il.Emit( System.Reflection.Emit.OpCodes.Ret );
		// The JIT may probe the address, so it has to be real memory of the struct's size.
		var buffer = Marshal.AllocHGlobal( SizeOf( f.DeclaringType! ) + 16 );
		try { return (int)(long)dm.Invoke( null, [buffer] )!; }
		finally { Marshal.FreeHGlobal( buffer ); }
	}

	static int SizeOf( Type t ) =>
		(int)typeof( Unsafe ).GetMethod( nameof( Unsafe.SizeOf ) )!.MakeGenericMethod( t ).Invoke( null, null )!;
}
