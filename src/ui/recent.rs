//! Panneau des dépôts récents avec recherche et filtrage.
//!
//! Affiche la liste des dépôts récemment ouverts depuis la configuration,
//! avec un champ de recherche, un bouton de navigation et des raccourcis rapides.

use std::collections::HashSet;
use std::path::Path;

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};

use crate::config::AppConfig;
use crate::ui::theme::Theme;

/// État du panneau des dépôts récents.
#[derive(Debug, Clone)]
pub struct RecentList {
    /// Texte saisi dans le champ de recherche (affiché immédiatement).
    pub search: String,
    /// Texte de recherche appliqué au filtrage (mis à jour après le délai).
    pub search_applied: String,
    /// Compteur de version pour le mécanisme de debounce.
    pub search_debounce: u32,
}

/// Messages émis par le panneau des dépôts récents.
#[derive(Debug, Clone)]
pub enum Message {
    /// Le texte du champ de recherche a changé.
    SearchChanged(String),
    /// Applique le filtre de recherche après le délai de debounce.
    DebouncedSearch(u32),
    /// L'utilisateur a cliqué sur le bouton « Parcourir... ».
    BrowseClicked,
    /// L'utilisateur a sélectionné un dépôt dans la liste.
    RepoSelected(String),
    /// L'utilisateur a cliqué sur un raccourci d'accès rapide.
    QuickAccessClicked(String),
    /// Résultat du dialogue de sélection de dossier.
    BrowseResult(Option<String>),
}

impl RecentList {
    /// Crée une nouvelle instance du panneau.
    pub fn new() -> Self {
        Self {
            search: String::new(),
            search_applied: String::new(),
            search_debounce: 0,
        }
    }

    /// Traite un message et renvoie une tâche Iced ainsi qu'un chemin
    /// de dépôt sélectionné le cas échéant.
    pub fn update(&mut self, message: Message) -> (Task<Message>, Option<String>) {
        match message {
            Message::SearchChanged(value) => {
                // Stocker le texte immédiatement pour l'affichage du champ
                self.search = value;
                // Incrémenter la version pour invalider les debounces précédents
                self.search_debounce += 1;
                let version = self.search_debounce;
                // Retourner une tâche qui attend 250ms puis envoie DebouncedSearch
                let task = Task::perform(
                    async move {
                        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    },
                    move |_| Message::DebouncedSearch(version),
                );
                (task, None)
            }
            Message::DebouncedSearch(version) => {
                // Appliquer uniquement si aucune frappe plus récente n'a eu lieu
                if version == self.search_debounce {
                    self.search_applied = self.search.clone();
                }
                (Task::none(), None)
            }
            Message::BrowseClicked => {
                // Ouvrir le dialogue de fichiers dans un thread bloquant.
                let task = Task::perform(
                    tokio::task::spawn_blocking(|| {
                        rfd::FileDialog::new()
                            .pick_folder()
                            .map(|p| p.display().to_string())
                    }),
                    |result| Message::BrowseResult(result.ok().flatten()),
                );
                (task, None)
            }
            Message::BrowseResult(Some(path)) => {
                self.search.clear();
                self.search_applied.clear();
                (Task::none(), Some(path))
            }
            Message::BrowseResult(None) => (Task::none(), None),
            Message::RepoSelected(path) => {
                self.search.clear();
                self.search_applied.clear();
                (Task::none(), Some(path))
            }
            Message::QuickAccessClicked(path) => {
                self.search.clear();
                self.search_applied.clear();
                (Task::none(), Some(path))
            }
        }
    }

    /// Construit la vue Iced du panneau des dépôts récents.
    pub fn view<'a>(&'a self, theme: &'a Theme, config: &'a AppConfig) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        // En-tête : titre et description.
        let heading = text("Recently opened")
            .size(typo.heading)
            .color(palette.text_primary);

        let description = text("Search and reopen workspaces you've used recently.")
            .size(typo.body)
            .color(palette.text_secondary);

        // Barre de recherche et bouton parcourir.
        let filter_label = text("Filter")
            .size(typo.body)
            .color(palette.text_secondary);

        let search_input = text_input("Type to filter by name or path", &self.search)
            .on_input(Message::SearchChanged)
            .width(Length::Fixed(320.0));

        let browse_btn = button(
            text("Browse...")
                .size(typo.body)
                .color(palette.text_primary),
        )
        .on_press(Message::BrowseClicked);

        let filter_row = row![filter_label, search_input, browse_btn]
            .spacing(theme.spacing.sm)
            .align_y(Alignment::Center);

        // Contenu principal : liste des dépôts ou message d'état vide.
        let content = if config.recent_repos().is_empty() {
            self.view_empty_state(theme)
        } else {
            self.view_repo_list(theme, config)
        };

        let body = scrollable(content).width(Length::Fill).height(Length::Fill);

        // Assemblage final de la colonne.
        column![heading, description, Space::with_height(8.0), filter_row, Space::with_height(8.0), body]
            .spacing(theme.spacing.xs)
            .width(Length::Fill)
            .into()
    }

    /// Construit la liste filtrée des dépôts récents.
    fn view_repo_list<'a>(&'a self, theme: &'a Theme, config: &'a AppConfig) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;
        let query = self.search_applied.to_lowercase();

        let mut items = column![].spacing(theme.spacing.xs);

        for entry in config.recent_repos() {
            let path = Path::new(&entry.path);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&entry.path);

            // Appliquer le filtre de recherche.
            let matches = query.is_empty()
                || entry.path.to_lowercase().contains(&query)
                || name.to_lowercase().contains(&query);

            if !matches {
                continue;
            }

            let label = column![
                text(name.to_string())
                    .size(typo.body)
                    .color(palette.text_primary),
                text(entry.path.clone())
                    .size(typo.label)
                    .color(palette.text_secondary),
            ]
            .spacing(2.0);

            let repo_btn = button(label)
                .width(Length::Fixed(520.0))
                .on_press(Message::RepoSelected(entry.path.clone()));

            items = items.push(repo_btn);
        }

        container(items).width(Length::Fill).into()
    }

    /// Construit la vue affichée lorsque la liste de dépôts récents est vide.
    fn view_empty_state<'a>(&'a self, theme: &'a Theme) -> Element<'a, Message> {
        let palette = &theme.palette;
        let typo = &theme.typography;

        let hint = text("Your recent repositories will appear here once you open a workspace.")
            .size(typo.body)
            .color(palette.text_secondary);

        let open_btn = button(
            text("Open a workspace folder")
                .size(typo.body)
                .color(palette.text_primary),
        )
        .width(Length::Fixed(520.0))
        .on_press(Message::BrowseClicked);

        let mut col = column![hint, Space::with_height(8.0), open_btn]
            .spacing(theme.spacing.xs);

        // Raccourcis d'accès rapide vers les répertoires courants.
        let common = Self::common_paths();
        if !common.is_empty() {
            col = col.push(Space::with_height(8.0));
            col = col.push(
                text("Quick access")
                    .size(typo.body)
                    .color(palette.text_secondary),
            );

            for (label, path) in common {
                let quick_btn = button(
                    text(format!("{label}: {path}"))
                        .size(typo.label)
                        .color(palette.text_secondary),
                )
                .on_press(Message::QuickAccessClicked(path));

                col = col.push(quick_btn);
            }
        }

        container(col).width(Length::Fill).into()
    }

    /// Renvoie les chemins courants disponibles sur le système (accueil, bureau, etc.).
    fn common_paths() -> Vec<(String, String)> {
        let mut paths = Vec::new();
        let mut seen = HashSet::new();

        let candidates = [
            ("Home", dirs::home_dir()),
            ("Desktop", dirs::desktop_dir()),
            ("Documents", dirs::document_dir()),
            ("Downloads", dirs::download_dir()),
        ];

        for (label, path) in candidates {
            if let Some(path) = path {
                if path.exists() {
                    let display = path.display().to_string();
                    if seen.insert(display.clone()) {
                        paths.push((label.to_string(), display));
                    }
                }
            }
        }

        paths
    }
}

impl Default for RecentList {
    fn default() -> Self {
        Self::new()
    }
}
