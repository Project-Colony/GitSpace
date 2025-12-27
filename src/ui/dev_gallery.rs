use eframe::egui::{self, RichText, Ui, Vec2};

use crate::ui::menu;
use crate::ui::theme::Theme;

const COMBO_OPTIONS: [&str; 3] = ["Option A", "Option B", "Option C"];

pub struct DevGalleryPanel {
    theme: Theme,
    toggled: bool,
    notifications_enabled: bool,
    slider_value: f32,
    text_input: String,
    combo_choice: usize,
}

impl DevGalleryPanel {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            toggled: false,
            notifications_enabled: true,
            slider_value: 0.35,
            text_input: String::from("Type here"),
            combo_choice: 0,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        ui.heading(RichText::new("UI Motion Gallery (dev only)").strong());
        ui.label(
            "Use this panel to visually verify motion effects, hover states, and focus styles across common UI elements.",
        );
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .id_source("dev-gallery-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.group(|ui| {
                    ui.label(RichText::new("Navigation + menus").strong());
                    ui.add_space(6.0);
                    menu::menu_item(
                        ui,
                        &self.theme,
                        ("dev-menu", "menu-item"),
                        "Sidebar item",
                        false,
                    );
                    menu::menu_item(
                        ui,
                        &self.theme,
                        ("dev-menu", "menu-item-selected"),
                        "Selected item",
                        true,
                    );
                    menu::menu_item_sized(
                        ui,
                        &self.theme,
                        ("dev-menu", "menu-item-disabled"),
                        RichText::new("Compact item"),
                        false,
                        Vec2::new(220.0, 28.0),
                        egui::Sense::click(),
                    );
                    ui.add_space(8.0);
                    ui.menu_button("Menu popup", |ui| {
                        menu::with_menu_popup_motion(ui, "dev-menu-popup", |ui| {
                            if menu::menu_item(
                                ui,
                                &self.theme,
                                ("dev-menu", "menu-action-1"),
                                "Menu action",
                                false,
                            )
                            .clicked()
                            {
                                ui.close_menu();
                            }
                            if menu::menu_item(
                                ui,
                                &self.theme,
                                ("dev-menu", "menu-action-2"),
                                "Secondary action",
                                false,
                            )
                            .clicked()
                            {
                                ui.close_menu();
                            }
                        });
                    });
                });

                ui.add_space(12.0);
                ui.group(|ui| {
                    ui.label(RichText::new("Buttons + toggles").strong());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_sized([140.0, 32.0], egui::Button::new("Primary"));
                        ui.add_sized([140.0, 32.0], egui::Button::new("Secondary"));
                    });
                    ui.add_space(6.0);
                    ui.checkbox(&mut self.toggled, "Enable feature");
                    ui.checkbox(
                        &mut self.notifications_enabled,
                        "Allow notifications",
                    );
                    ui.add(
                        egui::Slider::new(&mut self.slider_value, 0.0..=1.0)
                            .text("Intensity"),
                    );
                });

                ui.add_space(12.0);
                ui.group(|ui| {
                    ui.label(RichText::new("Inputs + selectors").strong());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("Text input");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.text_input)
                                .desired_width(220.0),
                        );
                    });
                    ui.add_space(6.0);
                    let combo_id = ui.make_persistent_id("dev-gallery-combo");
                    egui::ComboBox::from_id_source(combo_id)
                        .selected_text(COMBO_OPTIONS[self.combo_choice])
                        .icon(menu::combo_icon(self.theme.clone(), combo_id.with("icon")))
                        .show_ui(ui, |ui| {
                            for (index, label) in COMBO_OPTIONS.iter().enumerate() {
                                ui.selectable_value(&mut self.combo_choice, index, *label);
                            }
                        });
                });

                ui.add_space(12.0);
                ui.group(|ui| {
                    ui.label(RichText::new("Panel inventory (motion targets)").strong());
                    ui.add_space(4.0);
                    for line in [
                        "Clone panel: input fields, clone results list, per-row menus",
                        "Open/Recent list: list rows, hover states, pinned actions",
                        "Repo overview: summary cards, branch detail rows",
                        "Stage: file list rows, diff expansion toggles, commit menu",
                        "History: commit list rows, branch filter menu",
                        "Branches: row hover, context menu, pin/unpin actions",
                        "Auth: sign-in buttons, token input focus",
                        "Settings: tabs, toggles, theme/release comboboxes",
                        "Notifications: toast appearance, action buttons",
                    ] {
                        ui.label(format!("• {line}"));
                    }
                });
            });
    }
}
