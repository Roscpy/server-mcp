# mcp-vps-server

Serveur MCP léger et sécurisé (Rust) pour administration système sur VPS
et environnements mobiles (Termux). L'IA agit comme sysadmin assisté, sous
contraintes de sécurité strictes et validation humaine pour les actions
sensibles.

## État d'avancement

- [x] **Phase 1** — config TOML, audit JSON Lines, confinement de chemin
      anti path-traversal, tools `read_file`/`write_file` (backup auto)/`list_dir`
- [x] **Phase 2** — `exec_command` (whitelist stricte par égalité exacte +
      confirmation humaine bloquante), auth Bearer, canal de confirmation
      double (prompt MCP natif à finir de brancher — voir TODO dans
      `handler.rs` — + webhook Telegram fonctionnel)
- [x] **Phase 3** (base) — `tail_file` (allow-list de chemins),
      `service_logs` (journalctl), `snapshot_workspace`/`list_snapshots`.
      Le "self-healing itératif" de la roadmap n'est pas une boucle
      serveur : c'est le client MCP qui enchaîne exec_command → lit
      stderr → write_file → exec_command. Rollback automatique d'une
      snapshot : pas encore fait (voir note dans `snapshot.rs`).
- [x] **Phase 4** (base) — `get_resource_usage` (CPU/RAM/disque via
      `sysinfo`), seuils d'alerte codés en dur (`monitor::check_thresholds`,
      pas encore branché sur un tool ni sur une action bloquante)
- [ ] **Branchement rmcp non compilé** — le code suit les patterns
      confirmés de plusieurs serveurs publiés sur crates.io (macros
      `#[tool_router]`/`#[tool_handler]`, `StreamableHttpService`), mais
      n'a pas pu être compilé ici (pas d'accès réseau pour `cargo build`).
      Vérifie contre https://docs.rs/rmcp au premier build — attends-toi à
      des ajustements de détail (nom d'un champ de `ServerCapabilities`,
      etc.), pas de refonte d'architecture.
- [ ] Élicitation MCP native pour `exec_command` (actuellement Telegram
      uniquement câblé de bout en bout ; le prompt natif attend d'exposer
      `RequestContext` jusqu'à `ExecTools` — TODO explicite dans `handler.rs`)
- [ ] Whitelist par sous-commande (actuellement égalité exacte stricte,
      volontairement — voir note en tête de `exec.rs`)
- [ ] Utilisateur système dédié `mcp-runner` + règles `sudoers.d` (étape
      de déploiement, hors du binaire lui-même)
- [ ] Persistance de la whitelist dynamique (actuellement en mémoire
      seulement — un redémarrage revient à la whitelist de config.toml)

## Démarrage

```bash
cp config.example.toml config.toml
# éditer config.toml : bearer_token, workspace_root, transport (stdio|http)

cargo build --release
MCP_VPS_TOKEN="ton-token-secret" ./target/release/mcp-vps-server --config config.toml
```

## Sécurité

- Toute opération filesystem passe par `confine_path`, qui rejette
  `..`, les chemins absolus détournés et les liens symboliques sortant de
  `workspace_root`.
- `write_file` sauvegarde automatiquement l'ancien fichier (`.bak.<timestamp>`)
  avant écrasement, sauf si `backup_if_exists: false` est explicitement
  demandé par l'appelant.
- Chaque appel de tool est journalisé dans `audit.jsonl` (succès, refus,
  erreur), y compris les métadonnées suffisantes pour rejouer/auditer
  l'action a posteriori.
- Le process est censé tourner sous un compte dédié restreint
  (`mcp-runner`), jamais root — voir la section 2.2 de la roadmap pour la
  configuration `sudoers.d` ciblée (Phase 2).

## Licence

MIT OR Apache-2.0
# server-mcp
