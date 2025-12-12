use eframe::egui;

macro_rules! font_entry {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!(
                "../../assets/JetBrainsMonoNerdFont/",
                $name,
                ".ttf"
            )) as &[u8],
        )
    };
}

const FONT_FILES: &[(&str, &[u8])] = &[
    font_entry!("JetBrainsMonoNLNerdFont-Thin"),
    font_entry!("JetBrainsMonoNLNerdFont-ThinItalic"),
    font_entry!("JetBrainsMonoNLNerdFont-ExtraLight"),
    font_entry!("JetBrainsMonoNLNerdFont-ExtraLightItalic"),
    font_entry!("JetBrainsMonoNLNerdFont-Light"),
    font_entry!("JetBrainsMonoNLNerdFont-LightItalic"),
    font_entry!("JetBrainsMonoNLNerdFont-Regular"),
    font_entry!("JetBrainsMonoNLNerdFont-Italic"),
    font_entry!("JetBrainsMonoNLNerdFont-Medium"),
    font_entry!("JetBrainsMonoNLNerdFont-MediumItalic"),
    font_entry!("JetBrainsMonoNLNerdFont-SemiBold"),
    font_entry!("JetBrainsMonoNLNerdFont-SemiBoldItalic"),
    font_entry!("JetBrainsMonoNLNerdFont-Bold"),
    font_entry!("JetBrainsMonoNLNerdFont-BoldItalic"),
    font_entry!("JetBrainsMonoNLNerdFont-ExtraBold"),
    font_entry!("JetBrainsMonoNLNerdFont-ExtraBoldItalic"),
    font_entry!("JetBrainsMonoNerdFont-Thin"),
    font_entry!("JetBrainsMonoNerdFont-ThinItalic"),
    font_entry!("JetBrainsMonoNerdFont-ExtraLight"),
    font_entry!("JetBrainsMonoNerdFont-ExtraLightItalic"),
    font_entry!("JetBrainsMonoNerdFont-Light"),
    font_entry!("JetBrainsMonoNerdFont-LightItalic"),
    font_entry!("JetBrainsMonoNerdFont-Regular"),
    font_entry!("JetBrainsMonoNerdFont-Italic"),
    font_entry!("JetBrainsMonoNerdFont-Medium"),
    font_entry!("JetBrainsMonoNerdFont-MediumItalic"),
    font_entry!("JetBrainsMonoNerdFont-SemiBold"),
    font_entry!("JetBrainsMonoNerdFont-SemiBoldItalic"),
    font_entry!("JetBrainsMonoNerdFont-Bold"),
    font_entry!("JetBrainsMonoNerdFont-BoldItalic"),
    font_entry!("JetBrainsMonoNerdFont-ExtraBold"),
    font_entry!("JetBrainsMonoNerdFont-ExtraBoldItalic"),
    font_entry!("JetBrainsMonoNLNerdFontMono-Thin"),
    font_entry!("JetBrainsMonoNLNerdFontMono-ThinItalic"),
    font_entry!("JetBrainsMonoNLNerdFontMono-ExtraLight"),
    font_entry!("JetBrainsMonoNLNerdFontMono-ExtraLightItalic"),
    font_entry!("JetBrainsMonoNLNerdFontMono-Light"),
    font_entry!("JetBrainsMonoNLNerdFontMono-LightItalic"),
    font_entry!("JetBrainsMonoNLNerdFontMono-Regular"),
    font_entry!("JetBrainsMonoNLNerdFontMono-Italic"),
    font_entry!("JetBrainsMonoNLNerdFontMono-Medium"),
    font_entry!("JetBrainsMonoNLNerdFontMono-MediumItalic"),
    font_entry!("JetBrainsMonoNLNerdFontMono-SemiBold"),
    font_entry!("JetBrainsMonoNLNerdFontMono-SemiBoldItalic"),
    font_entry!("JetBrainsMonoNLNerdFontMono-Bold"),
    font_entry!("JetBrainsMonoNLNerdFontMono-BoldItalic"),
    font_entry!("JetBrainsMonoNLNerdFontMono-ExtraBold"),
    font_entry!("JetBrainsMonoNLNerdFontMono-ExtraBoldItalic"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-Thin"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-ThinItalic"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-ExtraLight"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-ExtraLightItalic"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-Light"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-LightItalic"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-Regular"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-Italic"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-Medium"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-MediumItalic"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-SemiBold"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-SemiBoldItalic"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-Bold"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-BoldItalic"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-ExtraBold"),
    font_entry!("JetBrainsMonoNLNerdFontPropo-ExtraBoldItalic"),
];

const DEFAULT_PROPORTIONAL_STACK: &[&str] = &[
    "JetBrainsMonoNerdFont-Regular",
    "JetBrainsMonoNerdFont-Medium",
    "JetBrainsMonoNerdFont-SemiBold",
    "JetBrainsMonoNerdFont-Bold",
    "JetBrainsMonoNerdFont-Italic",
    "JetBrainsMonoNerdFont-BoldItalic",
    "JetBrainsMonoNerdFontPropo-Regular",
    "JetBrainsMonoNerdFontPropo-Medium",
    "JetBrainsMonoNerdFontPropo-SemiBold",
    "JetBrainsMonoNerdFontPropo-Bold",
];

const DEFAULT_MONOSPACE_STACK: &[&str] = &[
    "JetBrainsMonoNerdFontMono-Regular",
    "JetBrainsMonoNerdFontMono-Medium",
    "JetBrainsMonoNerdFontMono-SemiBold",
    "JetBrainsMonoNerdFontMono-Bold",
    "JetBrainsMonoNerdFontMono-Italic",
    "JetBrainsMonoNerdFontMono-BoldItalic",
    "JetBrainsMonoNerdFont-Regular",
];

pub fn install_fonts(ctx: &egui::Context) {
    let mut definitions = egui::FontDefinitions::default();
    definitions.font_data.clear();

    for (name, data) in FONT_FILES.iter() {
        definitions.font_data.insert(
            (*name).to_string(),
            egui::FontData::from_owned(data.to_vec()),
        );
    }

    definitions.families.insert(
        egui::FontFamily::Proportional,
        DEFAULT_PROPORTIONAL_STACK
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
    );

    definitions.families.insert(
        egui::FontFamily::Monospace,
        DEFAULT_MONOSPACE_STACK
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
    );

    ctx.set_fonts(definitions);
}
