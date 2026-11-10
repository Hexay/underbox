//! `same_layout!`: an abi struct and its Box2D twin have the same size, alignment and field offsets.

macro_rules! same_layout {
    ($name:ident: $($ours:ident = $theirs:ident),* $(,)?) => {{
        type Ours = abi::structs::$name;
        type Theirs = box2d_sys::$name;
        assert_eq!(size_of::<Ours>(), size_of::<Theirs>(), concat!(stringify!($name), " size"));
        assert_eq!(align_of::<Ours>(), align_of::<Theirs>(), concat!(stringify!($name), " alignment"));
        $(assert_eq!(
            std::mem::offset_of!(Ours, $ours),
            std::mem::offset_of!(Theirs, $theirs),
            concat!(stringify!($name), ".", stringify!($theirs)),
        );)*
    }};
}

pub(super) use same_layout;
