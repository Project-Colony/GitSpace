//! Chargement des polices JetBrains Mono Nerd Font pour Iced.
//!
//! On ne charge que les poids essentiels (Regular, Medium, SemiBold, Bold
//! et leurs variantes Italic) pour réduire l'empreinte mémoire.

/// Données de police intégrées au binaire pour les poids essentiels.
pub const FONT_REGULAR: &[u8] = include_bytes!(
    "../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFont-Regular.ttf"
);
pub const FONT_MEDIUM: &[u8] = include_bytes!(
    "../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFont-Medium.ttf"
);
pub const FONT_SEMIBOLD: &[u8] = include_bytes!(
    "../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFont-SemiBold.ttf"
);
pub const FONT_BOLD: &[u8] = include_bytes!(
    "../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFont-Bold.ttf"
);
pub const FONT_ITALIC: &[u8] = include_bytes!(
    "../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFont-Italic.ttf"
);
pub const FONT_BOLD_ITALIC: &[u8] = include_bytes!(
    "../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFont-BoldItalic.ttf"
);

// Mono variants pour le code / diffs
pub const FONT_MONO_REGULAR: &[u8] = include_bytes!(
    "../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFontMono-Regular.ttf"
);
pub const FONT_MONO_BOLD: &[u8] = include_bytes!(
    "../../assets/JetBrainsMonoNerdFont/JetBrainsMonoNerdFontMono-Bold.ttf"
);

/// Police par défaut pour l'UI.
pub const DEFAULT_FONT: iced::Font = iced::Font {
    family: iced::font::Family::Name("JetBrainsMono Nerd Font"),
    weight: iced::font::Weight::Normal,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};

/// Police monospace pour le code et les diffs.
#[allow(dead_code)]
pub const MONO_FONT: iced::Font = iced::Font {
    family: iced::font::Family::Name("JetBrainsMono Nerd Font Mono"),
    weight: iced::font::Weight::Normal,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};

/// Police bold pour les titres et éléments accentués.
#[allow(dead_code)]
pub const BOLD_FONT: iced::Font = iced::Font {
    family: iced::font::Family::Name("JetBrainsMono Nerd Font"),
    weight: iced::font::Weight::Bold,
    stretch: iced::font::Stretch::Normal,
    style: iced::font::Style::Normal,
};
