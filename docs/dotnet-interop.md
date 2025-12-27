# .NET interop

Ce document décrit la communication JSON entre GitSpace (Rust) et le helper .NET, ainsi que la
taxonomie d'erreurs utilisée pour la télémétrie et l'UX.

## Format de requête

Le helper .NET accepte une requête JSON sur `stdin` et renvoie une réponse JSON sur `stdout`.

```json
{
  "id": "req-0001",
  "command": "dialog.open",
  "payload": {
    "kind": "open_folder",
    "title": "Sélectionner un dossier",
    "filters": [],
    "options": {
      "multi_select": false,
      "show_hidden": false
    }
  }
}
```

## Format de réponse

```json
{
  "id": "req-0001",
  "status": "ok",
  "payload": {
    "selected_paths": ["/tmp/example"],
    "cancelled": false
  },
  "error": null
}
```

En cas d'échec côté helper, `status` vaut `error` et `error` contient des détails.

```json
{
  "id": "req-0001",
  "status": "error",
  "payload": null,
  "error": {
    "category": "dialog",
    "message": "Native dialog failed to open",
    "details": {"os_error": "E_ACCESSDENIED"}
  }
}
```

## Taxonomie d'erreurs

Ces catégories sont utilisées côté Rust pour logger et exposer un message UX cohérent.

| Catégorie | Source | Description | Signal |
| --- | --- | --- | --- |
| `dotnet.helper.launch_failed` | Rust | Échec au lancement du helper .NET (`Command::spawn`). | Log `gitspace::telemetry` |
| `dotnet.helper.json_parse_failed` | Rust | Erreur de parsing JSON sur la réponse helper (ou payload). | Log `gitspace::telemetry` |
| `dotnet.helper.process_failed` | Rust | Le helper s'est terminé avec un code non nul. | Message UX + `AppError::Unknown` |
| `dotnet.helper.response_error` | Helper | Réponse `status = error` avec `error.category`/`error.message`. | Message UX |

## Exemple côté Rust

```rust
use crate::dotnet::{DialogOpenRequest, DialogOptions, DotnetClient};

let request = DialogOpenRequest {
    kind: "open_folder".to_string(),
    title: Some("Select default clone destination".to_string()),
    filters: Vec::new(),
    options: DialogOptions {
        multi_select: false,
        show_hidden: false,
    },
};

let response = DotnetClient::helper().dialog_open(request)?;
```

## Exemple côté shell

```bash
echo '{"id":"req-0001","command":"ping","payload":{}}' \
  | dotnet run --project dotnet/GitSpace.Helper/GitSpace.Helper.csproj
```

## Checklist de validation

- [ ] Version .NET (runtime compatible et accessible via `dotnet --version`) — bloqué : `dotnet` introuvable dans l'environnement.
- [ ] Démarrage (le helper répond à `ping` via stdin/stdout) — bloqué : `dotnet` introuvable, test `dotnet run` impossible.
- [ ] Erreurs (les réponses `status=error` remontent `category`/`message` attendus) — bloqué : `dotnet` introuvable, test `dotnet run` impossible.
- [ ] Cas d’usage (ex: `dialog.open` renvoie une charge utile cohérente)
