# mcp-vps-server



![CI](https://github.com/Roscpy/mcp-vps-server/actions/workflows/ci.yml/badge.svg)




![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)



Serveur **MCP (Model Context Protocol)** écrit en Rust pour laisser une IA administrer un VPS ou un terminal Termux, avec des garde-fous stricts : accès fichiers confiné, commandes en liste blanche, confirmation humaine pour le reste, journal d'audit complet.

Compatible avec tout client MCP supportant le transport HTTP (Claude Code, Claude Desktop, Cursor, Zed, agents personnalisés) ou stdio.

## Fonctionnalités

- **Fichiers confinés** : `read_file`, `write_file` (sauvegarde `.bak` automatique), `list_dir`, limités à un dossier racine. Les `..` et les liens symboliques sortants sont refusés.
- **Exécution de commandes** : liste blanche stricte (égalité exacte), pas d'interprétation shell (pas de pipes ni de redirections). Toute autre commande attend une approbation humaine.
- **Confirmation humaine** : bot Telegram avec boutons Approuver / Refuser. Si le client supporte l'élicitation MCP, la question lui est posée directement ; sinon repli sur Telegram.
- **Diagnostic** : lecture de logs (`tail_file` sur une liste de chemins autorisés, `service_logs` via journalctl).
- **Snapshots** : copie du workspace avant une opération risquée.
- **Monitoring** : CPU, RAM et disque.
- **Audit** : chaque action est écrite dans un fichier JSON Lines.
- **Léger et portable** : binaire Rust unique, testé sur Termux (Android aarch64) et prévu pour Ubuntu/Debian.

## Installation

Prérequis : Rust stable. Sur Termux : `pkg install rust clang make pkg-config binutils`.

```bash
git clone https://github.com/Roscpy/mcp-vps-server.git
cd mcp-vps-server
cargo build --release
```

## Configuration

Copie l'exemple et adapte-le :

```bash
cp config.example.toml config.toml
```

| Section | Clé | Rôle |
|---|---|---|
| `[server]` | `transport` | `"http"` (distant) ou `"stdio"` (local) |
| `[server]` | `bind_addr` | Adresse d'écoute HTTP, ex. `127.0.0.1:8787` |
| `[auth]` | `bearer_token` | Token attendu dans `Authorization: Bearer ...` |
| `[filesystem]` | `workspace_root` | Dossier racine (chemin absolu) auquel les outils fichiers sont confinés |
| `[audit]` | `log_path` | Fichier du journal d'audit |
| `[commands]` | `whitelist` | Commandes exécutables sans confirmation |
| `[telegram]` | `enabled`, `bot_token`, `chat_id` | Canal de confirmation externe |
| `[logs]` | `allowed_paths` | Fichiers lisibles via `tail_file` |
| `[snapshots]` | `dir` | Dossier des snapshots |

Le token peut aussi venir de la variable d'environnement `MCP_VPS_TOKEN`, qui est prioritaire. Le serveur refuse de démarrer avec le token par défaut.

Génération d'un token :

```bash
head -c 24 /dev/urandom | base64 | tr -d '/+='
```

## Lancement

```bash
./target/release/mcp-vps-server --config config.toml
# ou en forçant le transport :
./target/release/mcp-vps-server --config config.toml --transport stdio
```

Test rapide de l'authentification :

```bash
curl -i http://127.0.0.1:8787/mcp   # 401 attendu
```

## Connecter un client

Claude Code :

```bash
claude mcp add --transport http mcp-vps http://127.0.0.1:8787/mcp \
  --header "Authorization: Bearer TON_TOKEN"
```

Autres clients, configuration JSON typique (le format exact varie selon le client) :

```json
{
  "mcpServers": {
    "mcp-vps": {
      "type": "http",
      "url": "http://127.0.0.1:8787/mcp",
      "headers": { "Authorization": "Bearer TON_TOKEN" }
    }
  }
}
```

En mode stdio, le client lance directement le binaire avec `--transport stdio`.

## Outils exposés

| Outil | Description |
|---|---|
| `read_file` | Lit un fichier texte dans le workspace |
| `write_file` | Écrit un fichier, avec sauvegarde `.bak.<horodatage>` si il existe déjà |
| `list_dir` | Liste un dossier du workspace |
| `exec_command` | Exécute une commande (liste blanche, sinon confirmation humaine) |
| `tail_file` | Dernières lignes d'un fichier de log autorisé |
| `service_logs` | Logs d'un service systemd (`journalctl`, Linux avec systemd uniquement) |
| `snapshot_workspace` | Copie du workspace |
| `list_snapshots` | Liste des snapshots |
| `get_resource_usage` | CPU, RAM, disques |

## Sécurité

- **TLS** : le serveur parle HTTP simple. Ne l'expose pas directement sur Internet : écoute sur `127.0.0.1` et place-le derrière un reverse proxy TLS (Caddy, Nginx), sinon le token circule en clair.
- **Utilisateur dédié** : fais tourner le serveur sous un compte restreint (par exemple `mcp-runner`), jamais root. Pour les rares commandes nécessitant sudo, utilise des règles ciblées dans `/etc/sudoers.d/`.
- **Liste blanche stricte** : `"git"` n'autorise pas `"git config ..."`. Chaque commande autorisée est listée en entier.
- **Pas de shell** : les commandes sont découpées puis exécutées directement, ce qui évite les injections par pipes, `;` ou substitutions.
- **Audit** : une ligne JSON par action, avec le statut et si une confirmation était requise :

```json
{"id":"...","timestamp":"2026-09-28T01:25:47+00:00","tool":"write_file","params":{"bytes":5,"path":"hello.txt"},"outcome":{"status":"success","summary":"fichier écrit"},"requires_confirmation":false}
```

## Confirmation via Telegram

1. Crée un bot avec @BotFather et récupère le `bot_token`.
2. Récupère ton `chat_id` (message au bot puis `getUpdates`).
3. Renseigne la section `[telegram]` avec `enabled = true`.
4. Enregistre le webhook vers `https://ton-domaine/telegram/webhook` (URL HTTPS publique, donc plutôt sur un VPS derrière un reverse proxy).

Limite connue : le webhook ne vérifie pas encore le header `X-Telegram-Bot-Api-Secret-Token`. Les identifiants de confirmation sont des UUID v4 non devinables, mais ce durcissement reste à faire.

Sans Telegram activé et si le client ne supporte pas l'élicitation, une commande hors liste blanche attend 5 minutes puis est refusée.

## Termux (Android)

- Utilise des chemins sous `$HOME` pour `workspace_root`, `audit.log_path` et `snapshots.dir`.
- `service_logs` ne fonctionne pas (pas de systemd).
- Pas de root ni de sudo : la partie « privilèges élevés » ne s'applique qu'au VPS.

## Feuille de route

- [x] Fichiers confinés, audit JSON Lines, authentification Bearer
- [x] Exécution avec liste blanche et confirmation Telegram
- [x] Logs, snapshots, monitoring
- [x] Transports HTTP (streamable) et stdio
- [x] Confirmation native MCP (élicitation), avec repli sur Telegram
- [ ] Vérification du secret du webhook Telegram
- [ ] Restauration d'un snapshot (avec confirmation)
- [ ] Liste blanche par sous-commande et persistance des ajouts dynamiques
- [ ] Seuils de monitoring configurables et branchés sur `exec_command`
- [ ] Publication sur crates.io

## Contribuer

Les issues et pull requests sont bienvenues. Avant de proposer un changement :

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

## Licence

Double licence [MIT](LICENSE-MIT) ou [Apache-2.0](LICENSE-APACHE), au choix.
