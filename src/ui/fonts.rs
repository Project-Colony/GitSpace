//! JetBrains Mono Nerd Font, embedded in the binary.
//!
//! Only the Regular weight is bundled: no widget asks for another weight or style.

/// Regular weight, registered with iced at startup.
pub const FONT_REGULAR: &[u8] =
    include_bytes!("../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFont-Regular.ttf");

/// Default font of the interface. It is monospaced, so diffs and codes use it too.
pub const DEFAULT_FONT: iced::Font = iced::Font::with_name("JetBrainsMono Nerd Font");
