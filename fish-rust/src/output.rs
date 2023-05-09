use bitflags::bitflags;

bitflags! {
    pub struct ColorSupport: u8 {
        const NONE = 0;
        const TERM_256COLOR = 1<<0;
        const TERM_24BIT = 1<<1;
    }
}

extern "C" {
    pub fn output_set_color_support(value: libc::c_int);
}
