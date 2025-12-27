# Contrats .NET (IPC)

Ce document décrit les contrats d’échange entre le binaire Rust et le helper .NET via un canal IPC local.

## Endpoints / commands supportés

Tous les appels sont décrits comme un message JSON avec une enveloppe commune :

```json
{
  "id": "req-0001",
  "command": "dialog.open",
  "payload": {}
}
```

### `dialog.open`
Ouvre un dialogue natif (fichier, dossier, enregistrement).

**Payload (entrée)**
- `kind`: `open_file` | `open_folder` | `save_file`
- `title`: titre de la fenêtre
- `filters`: liste des filtres (`label`, `extensions`)
- `options`: options booléennes (`multi_select`, `show_hidden`)

**Réponse (sortie)**
- `selected_paths`: tableau de chemins
- `cancelled`: booléen

### `credential.request`
Demande un credential au provider système.

**Payload (entrée)**
- `service`: nom du service (ex: `github.com`)
- `account`: identifiant utilisateur (facultatif)
- `action`: `get` | `store` | `erase`

**Réponse (sortie)**
- `username`: identifiant utilisateur
- `secret`: secret (mot de passe, token)
- `status`: `ok` | `not_found` | `denied`

### `library.call`
Appelle une bibliothèque .NET spécifique (interop ciblée).

**Payload (entrée)**
- `name`: nom logique de la librairie/feature
- `payload`: objet JSON libre (contrat propre à la librairie)

**Réponse (sortie)**
- `payload`: objet JSON libre
- `status`: `ok` | `error`
- `error`: erreur structurée (si `status` = `error`)

### `ping`
Vérifie la disponibilité du helper .NET.

**Payload (entrée)**
- `{}`

**Réponse (sortie)**
- `status`: `ok`
- `version`: version du helper

## Format d’entrée/sortie

- **Encodage**: JSON UTF-8.
- **Enveloppe commune**: `{ id, command, payload }` côté requête, `{ id, status, payload, error }` côté réponse.
- **Exemples JSON**: voir le dossier [`/schemas`](../schemas).

Exemple de réponse :

```json
{
  "id": "req-0001",
  "status": "ok",
  "payload": {
    "selected_paths": ["/Users/alex/Documents/report.pdf"],
    "cancelled": false
  }
}
```

## Codes d’erreur

Les erreurs remontent toujours un objet structuré :

```json
{
  "category": "InvalidRequest",
  "message": "Missing payload.kind",
  "details": {
    "field": "kind"
  }
}
```

| Catégorie | Quand ? | Message attendu |
| --- | --- | --- |
| `Cancelled` | L’utilisateur annule l’action. | "User cancelled" |
| `Unavailable` | Helper .NET indisponible ou fonctionnalité non supportée. | "Helper unavailable" |
| `InvalidRequest` | Paramètres manquants ou invalides. | "Missing payload.kind" |
| `Internal` | Erreur interne .NET. | "Unhandled exception" |

Les messages doivent rester courts et exploitables côté UI (Rust). Les détails supplémentaires peuvent être placés dans `details`.
