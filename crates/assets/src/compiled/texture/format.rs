//! `VTexFormat` values (VTexFormat.cs) and the bytes a mip takes in each.

/// A `VTexFormat` byte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Format(pub u8);

impl Format {
    pub const DXT1: Self = Self(1);
    pub const DXT5: Self = Self(2);
    pub const I8: Self = Self(3);
    pub const RGBA8888: Self = Self(4);
    pub const R16: Self = Self(5);
    pub const RG1616: Self = Self(6);
    pub const RGBA16161616: Self = Self(7);
    pub const R16F: Self = Self(8);
    pub const RG1616F: Self = Self(9);
    pub const RGBA16161616F: Self = Self(10);
    pub const R32F: Self = Self(11);
    pub const RG3232F: Self = Self(12);
    pub const RGB323232F: Self = Self(13);
    pub const RGBA32323232F: Self = Self(14);
    pub const JPEG_RGBA8888: Self = Self(15);
    pub const PNG_RGBA8888: Self = Self(16);
    pub const JPEG_DXT5: Self = Self(17);
    pub const PNG_DXT5: Self = Self(18);
    pub const BC6H: Self = Self(19);
    pub const BC7: Self = Self(20);
    pub const ATI2N: Self = Self(21);
    pub const IA88: Self = Self(22);
    pub const ETC2: Self = Self(23);
    pub const ETC2_EAC: Self = Self(24);
    pub const R11_EAC: Self = Self(25);
    pub const RG11_EAC: Self = Self(26);
    pub const ATI1N: Self = Self(27);
    pub const BGRA8888: Self = Self(28);
    pub const WEBP_RGBA8888: Self = Self(29);
    pub const WEBP_DXT5: Self = Self(30);
    pub const R32_UINT: Self = Self(31);
    pub const A8: Self = Self(32);
    pub const R8_UNORM: Self = Self(33);

    /// Bytes per 4x4 block, for block-compressed formats.
    pub fn block_bytes(self) -> Option<usize> {
        match self {
            Self::DXT1 | Self::ATI1N | Self::ETC2 | Self::R11_EAC => Some(8),
            Self::DXT5 | Self::BC6H | Self::BC7 | Self::ATI2N | Self::ETC2_EAC | Self::RG11_EAC => Some(16),
            _ => None,
        }
    }

    /// Bytes per texel, for the others.
    pub fn texel_bytes(self) -> usize {
        match self {
            Self::R16 | Self::R16F | Self::IA88 => 2,
            Self::RGBA8888 | Self::BGRA8888 | Self::RG1616 | Self::RG1616F | Self::R32F | Self::R32_UINT => 4,
            Self::RGBA16161616 | Self::RGBA16161616F | Self::RG3232F => 8,
            Self::RGB323232F => 12,
            Self::RGBA32323232F => 16,
            _ => 1,
        }
    }

    /// The pixels are one JPEG/PNG/WebP file rather than mips.
    pub fn is_image_file(self) -> bool {
        matches!(self, Self::JPEG_RGBA8888 | Self::PNG_RGBA8888 | Self::JPEG_DXT5 | Self::PNG_DXT5 | Self::WEBP_RGBA8888 | Self::WEBP_DXT5)
    }

    /// Linear data that needs more than 8 bits (decodes to RGBA16F).
    pub fn is_hdr(self) -> bool {
        matches!(self.0, 5..=14 | 19)
    }
}

/// Bytes of a `w`x`h` mip with `layers` faces/slices. Block formats round up to whole blocks and,
/// as VRF does, count 2 or 3 layers as 4.
pub fn mip_size(format: Format, w: usize, h: usize, layers: usize) -> usize {
    match format.block_bytes() {
        Some(block) => {
            let layers = if layers > 1 && layers < 4 { 4 } else { layers };
            (w.next_multiple_of(4).max(4) * h.next_multiple_of(4).max(4) / 16) * layers * block
        }
        None => w * h * layers * format.texel_bytes(),
    }
}
