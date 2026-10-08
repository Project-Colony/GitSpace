//! Panneau de demonstration des composants UI (developpement uniquement).
//!
//! Affiche une galerie de widgets Iced pour verifier visuellement les styles,
//! les etats de survol et les interactions sur les elements courants.

use iced::widget::{
    button, checkbox, column, container, horizontal_rule, pick_list, row, scrollable, slider, text,
    text_input, Space,
};
use iced::{Alignment, Element, Length, Task};

use crate::ui::theme::Theme;

/// Options du selecteur de demonstration.
const COMBO_OPTIONS: [&str; 3] = ["Option A", "Option B", "Option C"];

/// Messages emis par le panneau de galerie.
#[derive(Debug, Clone)]
pub enum Message {
    /// La case a cocher « Activer la fonctionnalite » a change.
    FeatureToggled(bool),
    /// La case a cocher « Notifications » a change.
    NotificationsToggled(bool),
    /// La valeur du curseur d'intensite a change.
    SliderChanged(f32),
    /// Le texte du champ de saisie a change.
    TextInputChanged(String),
    /// L'option du selecteur a change.
    ComboChanged(String),
    /// Bouton « Primaire » presse.
    PrimaryPressed,
    /// Bouton « Secondaire » presse.
    SecondaryPressed,
    /// Le curseur de la deuxieme demonstration a change.
    DemoSlider2Changed(f32),
    /// La case a cocher supplementaire a change.
    ExtraCheckToggled(bool),
}

/// Etat du panneau de galerie de composants.
#[derive(Debug, Clone)]
pub struct DevGalleryPanel {
    /// Etat de la case a cocher de fonctionnalite.
    toggled: bool,
    /// Etat de la case a cocher de notifications.
    notifications_enabled: bool,
    /// Valeur du curseur d'intensite (0.0 a 1.0).
    slider_value: f32,
    /// Texte du champ de saisie.
    text_input_value: String,
    /// Index de l'option selectionnee dans le pick_list.
    combo_choice: String,
    /// Valeur du deuxieme curseur de demonstration.
    demo_slider_2: f32,
    /// Etat d'une case a cocher supplementaire.
    extra_check: bool,
}

impl DevGalleryPanel {
    /// Cree un nouveau panneau de galerie avec les valeurs par defaut.
    pub fn new() -> Self {
        Self {
            toggled: false,
            notifications_enabled: true,
            slider_value: 0.35,
            text_input_value: String::from("Saisissez du texte ici"),
            combo_choice: COMBO_OPTIONS[0].to_string(),
            demo_slider_2: 0.5,
            extra_check: false,
        }
    }

    /// Traite un message et renvoie une tache Iced.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FeatureToggled(val) => {
                self.toggled = val;
            }
            Message::NotificationsToggled(val) => {
                self.notifications_enabled = val;
            }
            Message::SliderChanged(val) => {
                self.slider_value = val;
            }
            Message::TextInputChanged(val) => {
                self.text_input_value = val;
            }
            Message::ComboChanged(val) => {
                self.combo_choice = val;
            }
            Message::PrimaryPressed | Message::SecondaryPressed => {
                // Pas d'action, demonstration visuelle uniquement.
            }
            Message::DemoSlider2Changed(val) => {
                self.demo_slider_2 = val;
            }
            Message::ExtraCheckToggled(val) => {
                self.extra_check = val;
            }
        }
        Task::none()
    }

    /// Construit la vue Iced de la galerie de composants.
    pub fn view<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        // En-tete.
        let heading = text("Galerie de composants UI (dev uniquement)")
            .size(typo.heading)
            .color(palette.text_primary);

        let description = text(
            "Utilisez ce panneau pour verifier visuellement les effets de mouvement, \
             les etats de survol et les styles de focus sur les elements courants.",
        )
        .size(typo.body)
        .color(palette.text_secondary);

        // Groupe : boutons et bascules.
        let buttons_group = self.view_buttons_group(theme);

        // Groupe : champs de saisie et selecteurs.
        let inputs_group = self.view_inputs_group(theme);

        // Groupe : curseurs et cases a cocher supplementaires.
        let sliders_group = self.view_sliders_group(theme);

        // Groupe : inventaire des panneaux.
        let inventory_group = self.view_panel_inventory(theme);

        let content = column![
            heading,
            description,
            Space::with_height(theme.spacing.md),
            buttons_group,
            Space::with_height(theme.spacing.md),
            inputs_group,
            Space::with_height(theme.spacing.md),
            sliders_group,
            Space::with_height(theme.spacing.md),
            inventory_group,
        ]
        .spacing(theme.spacing.xs)
        .width(Length::Fill);

        scrollable(container(content).width(Length::Fill).padding(theme.spacing.md))
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Groupe de demonstration des boutons et cases a cocher.
    fn view_buttons_group<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let group_title = text("Boutons et bascules")
            .size(typo.title)
            .color(palette.text_primary);

        let primary_btn = button(
            text("Primaire")
                .size(typo.body)
                .color(palette.text_primary),
        )
        .on_press(Message::PrimaryPressed)
        .width(Length::Fixed(140.0));

        let secondary_btn = button(
            text("Secondaire")
                .size(typo.body)
                .color(palette.text_secondary),
        )
        .on_press(Message::SecondaryPressed)
        .width(Length::Fixed(140.0));

        let btn_row = row![primary_btn, secondary_btn]
            .spacing(theme.spacing.sm);

        let feature_cb = checkbox("Activer la fonctionnalite", self.toggled)
            .on_toggle(Message::FeatureToggled);

        let notif_cb = checkbox("Autoriser les notifications", self.notifications_enabled)
            .on_toggle(Message::NotificationsToggled);

        let intensity_slider = slider(0.0..=1.0, self.slider_value, Message::SliderChanged)
            .width(Length::Fixed(200.0));

        let intensity_label = text(format!("Intensite : {:.2}", self.slider_value))
            .size(typo.body)
            .color(palette.text_secondary);

        let slider_row = row![intensity_label, intensity_slider]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        column![
            group_title,
            horizontal_rule(1),
            Space::with_height(theme.spacing.xs),
            btn_row,
            Space::with_height(theme.spacing.xs),
            feature_cb,
            notif_cb,
            slider_row,
        ]
        .spacing(theme.spacing.xs)
        .into()
    }

    /// Groupe de demonstration des champs de saisie et selecteurs.
    fn view_inputs_group<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let group_title = text("Champs de saisie et selecteurs")
            .size(typo.title)
            .color(palette.text_primary);

        let input_label = text("Saisie de texte")
            .size(typo.body)
            .color(palette.text_secondary);

        let input_field = text_input("Saisissez quelque chose...", &self.text_input_value)
            .on_input(Message::TextInputChanged)
            .width(Length::Fixed(220.0));

        let input_row = row![input_label, input_field]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let combo_label = text("Selecteur")
            .size(typo.body)
            .color(palette.text_secondary);

        let options: Vec<String> = COMBO_OPTIONS.iter().map(|s| s.to_string()).collect();
        let combo = pick_list(
            options,
            Some(self.combo_choice.clone()),
            Message::ComboChanged,
        )
        .width(Length::Fixed(160.0));

        let combo_row = row![combo_label, combo]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        column![
            group_title,
            horizontal_rule(1),
            Space::with_height(theme.spacing.xs),
            input_row,
            Space::with_height(theme.spacing.xs),
            combo_row,
        ]
        .spacing(theme.spacing.xs)
        .into()
    }

    /// Groupe de demonstration des curseurs et cases supplementaires.
    fn view_sliders_group<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let group_title = text("Curseurs supplementaires")
            .size(typo.title)
            .color(palette.text_primary);

        let slider_2 = slider(0.0..=1.0, self.demo_slider_2, Message::DemoSlider2Changed)
            .width(Length::Fixed(200.0));

        let slider_label = text(format!("Valeur : {:.2}", self.demo_slider_2))
            .size(typo.body)
            .color(palette.text_secondary);

        let slider_row = row![slider_label, slider_2]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        let extra_cb = checkbox("Option supplementaire", self.extra_check)
            .on_toggle(Message::ExtraCheckToggled);

        column![
            group_title,
            horizontal_rule(1),
            Space::with_height(theme.spacing.xs),
            slider_row,
            extra_cb,
        ]
        .spacing(theme.spacing.xs)
        .into()
    }

    /// Groupe d'inventaire des panneaux et cibles de mouvement.
    fn view_panel_inventory<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let group_title = text("Inventaire des panneaux (cibles de mouvement)")
            .size(typo.title)
            .color(palette.text_primary);

        let items = [
            "Clone : champs de saisie, liste de resultats, menus par ligne",
            "Ouvrir/Recents : lignes de liste, etats de survol, actions epinglees",
            "Vue du depot : cartes de resume, lignes de detail de branche",
            "Staging : lignes de fichiers, bascules d'expansion diff, menu de commit",
            "Historique : lignes de commits, menu de filtre de branche",
            "Branches : survol de ligne, menu contextuel, actions epingler/desepingler",
            "Auth : boutons de connexion, focus du champ de token",
            "Parametres : onglets, bascules, selecteurs de theme/release",
            "Notifications : apparition de toast, boutons d'action",
        ];

        let mut col = column![group_title, horizontal_rule(1), Space::with_height(theme.spacing.xs)]
            .spacing(theme.spacing.xs);

        for item in &items {
            col = col.push(
                text(format!("  - {item}"))
                    .size(typo.body)
                    .color(palette.text_secondary),
            );
        }

        col.into()
    }
}

impl Default for DevGalleryPanel {
    fn default() -> Self {
        Self::new()
    }
}
