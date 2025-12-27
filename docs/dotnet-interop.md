# Interop .NET

## Besoins .NET (depuis `docs/roadmap.md` et `tasks/tasks.md`)
- Dialogs natifs multiplateformes (sélecteurs de fichiers/dossiers, confirmations système).
- Credential providers intégrés à la plateforme.
- Réutilisation de bibliothèques existantes déjà disponibles côté .NET.

## Périmètre .NET
- **Reste en Rust**: UI principale, orchestration applicative, modèles métier, gestion Git, persistance locale.
- **Migre en .NET**: surfaces nécessitant des composants système natifs (dialogs), intégrations de credential providers, appels vers bibliothèques .NET spécifiques quand elles évitent de réimplémenter des fonctionnalités existantes.

## Mode d’appel retenu
- **IPC local** entre le binaire Rust et un helper .NET dédié.
  - Processus .NET lancé à la demande par Rust.
  - Contrat minimal et stable pour limiter la surface d’interop.
  - IPC choisi pour éviter les contraintes d’ABI/FFI entre runtimes.

## Modèle de données
- **Format d’échange**: JSON encodé en UTF-8 (facile à inspecter et à déboguer).
- **Structures échangées**:
  - `DialogRequest { kind, title, filters, options }`
  - `DialogResponse { selected_paths, cancelled }`
  - `CredentialRequest { service, account, action }`
  - `CredentialResponse { username, secret, status }`
  - `LibraryCall { name, payload }`
  - `LibraryResult { payload, status, error }`
- **Erreurs**:
  - Codes normalisés (`Cancelled`, `Unavailable`, `InvalidRequest`, `Internal`).
  - Messages d’erreur détaillés côté .NET, résumés côté Rust pour l’UI.
  - Mapping systématique vers un `Result` Rust (succès/échec) avec contexte.

## Notes d’évolution
- Si les contrats se stabilisent, évaluer MessagePack pour réduire la taille des messages.
- Documenter la stratégie de versioning du protocole et la compatibilité ascendante.
