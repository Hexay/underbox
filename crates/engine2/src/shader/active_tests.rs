//! `IsUiVariableActive`: expressions and declarations copied from sbox-public's core shaders.

use super::expr::{self, Scope};
use super::*;
use std::collections::HashMap;

struct Vars(HashMap<&'static str, i64>);

impl Scope for Vars {
    fn defined(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    fn value(&self, name: &str, _: u32) -> i64 {
        self.0.get(name).copied().unwrap_or(0)
    }
}

#[test]
fn evaluates_core_shader_conditions() {
    let vars = Vars(HashMap::from([
        ("PROGRAM", 2),
        ("VFX_PROGRAM_PS", 2),
        ("VFX_PROGRAM_CS", 6),
        ("S_SUBSURFACE_SCATTERING", 1),
        ("SUBSURFACE_SCATTERING_PREINTEGRATED", 1),
        ("S_MULTIBLEND", 2),
        ("S_SPECULAR", 1),
        ("CMAA_MSAA_SAMPLE_COUNT", 4),
        ("A_GPU", 1),
    ]));
    let cases = [
        ("( ( PROGRAM == VFX_PROGRAM_PS ) )", 1),
        ("!defined( PROGRAM ) || PROGRAM == VFX_PROGRAM_CS", 0),
        ("( S_SUBSURFACE_SCATTERING == SUBSURFACE_SCATTERING_PREINTEGRATED )", 1),
        ("S_MULTIBLEND >= 2", 1),
        ("S_MULTIBLEND >= 3", 0),
        ("( S_SPECULAR && !S_METALNESS_TEXTURE )", 1),
        ("(!S_UNLIT)", 1),
        ("defined( MATERIAL_CONTROLLED_BACKFACES ) || S_RENDER_BACKFACES", 0),
        ("defined(A_GPU)&&defined(A_HALF)", 0),
        ("CMAA_MSAA_SAMPLE_COUNT > 1", 1),
        ("XE_GTAO_USE_DEFAULT_CONSTANTS != 0", 0),
        ("( D_BLENDMODE == 3 )", 0),
        ("1 + 2 * 3 == 7 && -1 < 0 && 0x10 >> 4 == 1 ? 5 : 6", 5),
        ("MISSING_FN( 1, 2 ) || 7 / 0", 0),
    ];
    for (e, want) in cases {
        assert_eq!(expr::eval(e, &vars, 0), want, "{e}");
    }
}

/// vr_shared_standard_ps_code.fxc's colour and normal sections, as complex.shader includes them.
const STANDARD: &str = "FEATURES {\n\
    Feature( F_TRANSLUCENT, 0..1, \"Rendering\" );\n\
    Feature( F_USE_BENT_NORMALS, 0..1, \"Normal\" );\n\
}\n\
PS {\n\
    StaticCombo( S_TRANSLUCENT, F_TRANSLUCENT, Sys( ALL ) );\n\
    StaticCombo( S_ANISOTROPIC_GLOSS, 0..1, Sys( ALL ) );\n\
    BENT_DEFINE\n\
    #define ENABLE_NORMAL_MAPS\n\
    CreateInputTexture2D( TextureColor, Srgb, 8, \"\", \"_color\", \"Color/10\", Default3( 1.0, 1.0, 1.0 ) );\n\
    CreateInputTexture2D( TextureTranslucency, Linear, 8, \"\", \"_trans\", \"Translucent,15\", Default3( 1.0, 1.0, 1.0 ) );\n\
    #if ( S_TRANSLUCENT )\n\
        Texture2D g_tColor < Channel( RGB, AlphaWeighted( TextureColor, TextureTranslucency ), Srgb );\n\
                             Channel( A, Box( TextureTranslucency ), Linear ); OutputFormat( BC7 ); SrgbRead( true ); >;\n\
    #elif ( S_ALPHA_TEST )\n\
        Texture2D g_tColor < Channel( RGB, AlphaWeighted( TextureColor, TextureTranslucency ), Srgb ); OutputFormat( BC7 ); >;\n\
    #else\n\
        Texture2D g_tColor2 < Channel( RGB, Box( TextureColor ), Srgb ); OutputFormat( DXT1 ); SrgbRead( true ); >;\n\
    #endif\n\
    #ifdef ENABLE_NORMAL_MAPS\n\
        CreateInputTexture2D( TextureNormal, Linear, 8, \"NormalizeNormals\", \"_normal\", \"Normal,20\", Default3( 0.5, 0.5, 1.0 ) );\n\
        CreateInputTexture2D( TextureBentNormal, Linear, 8, \"NormalizeNormals\", \"_bentnormal\", \"Normal/20\", Default3( 0.5, 0.5, 1.0 ) );\n\
        #if ( S_ANISOTROPIC_GLOSS )\n\
            Texture2D g_tAnisoGloss < Channel( RG, AnisoRoughness_RG( TextureNormal ), Linear ); OutputFormat( ATI2N ); >;\n\
        #else\n\
            Texture2D g_tNormal < Channel( RGBA, HemiOctIsoRoughness_RG_B( TextureNormal ), Linear ); OutputFormat( BC7 ); SrgbRead( false ); > ;\n\
        #endif\n\
        #ifndef BENT_NORMAL_ACTIVE_IF\n\
        #define BENT_NORMAL_ACTIVE_IF\n\
        #endif\n\
        Texture2D g_tBentNormal < Channel( RG, HemiOctIsoRoughness_RG_B( TextureBentNormal ), Linear );  OutputFormat( ATI2N ); SrgbRead( false ); BENT_NORMAL_ACTIVE_IF >;\n\
        float g_flNormalMapScaleFactor < Default( 1.0 ); Range( 0.0, 1.0 ); UiGroup( \"Normal\" ); >;\n\
    #endif\n\
    float4 MainPs() : SV_Target0 { return 0; }\n\
}\n";

fn standard(bent_define: &str) -> VfxShader {
    VfxShader::parse(&STANDARD.replace("BENT_DEFINE", bent_define), Path::new("complex.shader"), &IncludePaths::default()).unwrap()
}

#[test]
fn bent_normal_input_follows_its_active_if_macro() {
    let s = standard("#define BENT_NORMAL_ACTIVE_IF ActiveIf( F_USE_BENT_NORMALS );");
    let active = |v, f, x| schema::ui_variable_active(&s, v, f, x);
    assert!(!active("TextureBentNormal", "F_USE_BENT_NORMALS", 0));
    assert!(active("TextureBentNormal", "F_USE_BENT_NORMALS", 1));
    assert!(active("TextureColor", "F_USE_BENT_NORMALS", 0), "unconditional");
    assert!(active("TextureNormal", "F_USE_BENT_NORMALS", 0), "under a live #ifdef, read by the #else texture");
    assert!(active("g_flNormalMapScaleFactor", "F_USE_BENT_NORMALS", 0));
    assert!(!active("TextureBentNormal", "F_USE_BENT_NORMALS", 2), "out of range");
    let fallback = standard("");
    assert!(schema::ui_variable_active(&fallback, "TextureBentNormal", "F_USE_BENT_NORMALS", 0), "#ifndef's empty ActiveIf");
}

#[test]
fn inputs_need_a_live_texture_reading_them() {
    let s = standard("");
    assert!(!schema::ui_variable_active(&s, "TextureTranslucency", "F_TRANSLUCENT", 0), "only the #if branch reads it");
    assert!(schema::ui_variable_active(&s, "TextureTranslucency", "F_TRANSLUCENT", 1));
    assert!(schema::ui_variable_active(&s, "TextureColor", "F_TRANSLUCENT", 1), "read by both branches");
}

#[test]
fn active_if_expressions_combine_features() {
    let src = "FEATURES { Feature( F_X, 0..2, \"G\" ); Feature( F_Y, 0..1, \"G\" ); }\n\
        PS { StaticCombo( S_X, F_X, Sys( ALL ) );\n\
        float g_flA < UiGroup( \"G\" ); ActiveIf( S_X == 2 && F_Y ); >;\n\
        float g_flB < UiGroup( \"G\" ); ActiveIf( !( F_X || F_Y ) ); >;\n\
        float4 MainPs() : SV_Target0 { return 0; } }\n";
    let s = VfxShader::parse(src, Path::new("t.shader"), &IncludePaths::default()).unwrap();
    let shown = |x, y| active::shown(&s, &HashMap::from([("F_X", x), ("F_Y", y)]));
    assert!(shown(2, 1).contains("g_flA") && !shown(1, 1).contains("g_flA") && !shown(2, 0).contains("g_flA"));
    assert!(shown(0, 0).contains("g_flB") && !shown(0, 1).contains("g_flB"));
}

#[test]
fn foliage_variables_follow_their_static_combos() {
    let Some(core) = crate::render::shading::tests::core_dir() else { return eprintln!("skipped: no sbox-public") };
    let s = VfxShader::parse_file(&core.join("shaders/foliage.shader"), &IncludePaths::new([core.clone()])).unwrap();
    let cases = [
        ("g_flGrazingFadeStart", "F_GRAZING_FADE"),
        ("g_flAlphaDistanceStart", "F_ALPHA_TEST"),
        ("TextureTransmissiveColor", "F_TRANSMISSIVE"),
        ("g_flTransmissionScale", "F_TRANSMISSIVE"),
        ("g_flEdgeFrequency", "F_FOLIAGE_ANIMATION"),
    ];
    for (var, feature) in cases {
        assert!(!schema::ui_variable_active(&s, var, feature, 0), "{var} hidden at {feature} 0");
        assert!(schema::ui_variable_active(&s, var, feature, 1), "{var} shown at {feature} 1");
    }
    for var in ["TextureColor", "TextureNormal", "g_flWrapAmount"] {
        assert!(schema::ui_variable_active(&s, var, "F_GRAZING_FADE", 0), "{var} is unconditional");
    }
}
