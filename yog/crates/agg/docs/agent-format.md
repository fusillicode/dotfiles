# Generic subagent format: capabilities and vendor mappings

Status: instruction and individual skill linking and subagent generation implemented.
Official agent documentation was refreshed on 2026-10-25. No vendor runtime was tested.

Supply the source directory and vendors as command inputs:

```console
agg sync-harness /path/to/ai-config codex,claude,cursor,gemini,opencode
```

Maintain the configuration once in that directory:

```text
ai-config/
├── AGENTS.md
├── skills/
└── subagents/
    └── reviewer.md
```

For each supplied vendor, the command has two operations:

1. Link `<src_dir>/AGENTS.md` and link skills from `<src_dir>/skills/` one by one
   to each vendor's default locations.
   Shared support directories are linked too; regular files in `skills/` are skipped.
   An existing link to the same source is a no-op. An existing link to a different
   source produces a CLI warning, remains unchanged, and does not stop other work.
2. Read generic definitions from `<src_dir>/subagents/` and generate vendor-specific
   agent files directly in each vendor's personal agent directory. Rerunning the
   command regenerates the corresponding files from the source definitions.

```text
Source AGENTS.md and skills/*/ -> individual symlinks in vendor configuration locations
Source subagents/              -> generate vendor-specific subagent files
```

The supplied source is authoritative. Other agent files are left alone. Vendor
selection belongs to the command input, not individual agent definitions.

The generic subagent format has no versioning.

## Instruction and skill destinations

| ID | Product | Instruction file | Individual skill directory |
| --- | --- | --- | --- |
| 1 | Codex | `~/.codex/AGENTS.md` | `~/.codex/skills/<name>` |
| 2 | Claude Code | `~/.claude/CLAUDE.md` | `~/.claude/skills/<name>` |
| 3 | Cursor | `~/.cursor/rules/agents.mdc` (generated mirror) | `~/.cursor/skills/<name>` |
| 4 | Gemini CLI | `~/.gemini/GEMINI.md` | `~/.gemini/skills/<name>` |
| 5 | OpenCode | `~/.config/opencode/AGENTS.md` | `~/.config/opencode/skills/<name>` |

These are default locations under `HOME`; vendor-specific environment overrides
are not applied. Missing `AGENTS.md`, `skills/`, and `subagents/` are skipped. Cursor instructions use
the existing personal rule format: YAML frontmatter with `description` and
`alwaysApply: true`, a Cursor-vendor-specific invocation/path preamble, then the source body.
Relative instruction paths resolve from the supplied source directory. The rule's
description names the authoritative source. Reruns overwrite an existing regular
Cursor rule when its contents differ, regardless of its previous source or header.
Cursor rule symlinks and directories are preserved with warnings.
Other existing instruction files and directories are preserved with warnings. An existing real skills
directory is merged; a conflicting symlink at that directory is skipped.

Sources: [Codex customization](https://learn.chatgpt.com/docs/customization/overview),
[Claude memory](https://code.claude.com/docs/en/memory) and
[skills](https://code.claude.com/docs/en/skills),
[Cursor rules](https://cursor.com/docs/rules) and [skills](https://cursor.com/docs/skills),
[Gemini context](https://geminicli.com/docs/cli/gemini-md/) and
[skills](https://geminicli.com/docs/cli/skills/),
[OpenCode rules](https://opencode.ai/docs/rules/) and [skills](https://opencode.ai/docs/skills/).

The Cursor mirror format was also checked directly against the existing
`~/.cursor/rules/agents.mdc` file.

## Vendor agent formats and default locations

| ID | Product | Agent format | Personal directory | Project directory |
| --- | --- | --- | --- | --- |
| 1 | [Codex][codex] | TOML | `~/.codex/agents/` | `.codex/agents/` |
| 2 | [Claude Code][claude] | Markdown with YAML frontmatter | `~/.claude/agents/` | `.claude/agents/` |
| 3 | [Cursor][cursor] | Markdown with YAML frontmatter | `~/.cursor/agents/` | `.cursor/agents/` |
| 4 | [Gemini CLI][gemini] | Markdown with YAML frontmatter | `~/.gemini/agents/` | `.gemini/agents/` |
| 5 | [OpenCode][opencode] | Markdown with YAML frontmatter | `~/.config/opencode/agents/` | `.opencode/agents/` |

Codex requires `name`, `description`, and `developer_instructions`. A standalone
agent file needs no registration stanza. Its prompt is a TOML string:

```toml
name = "reviewer"
description = "Reviews changes for correctness."
developer_instructions = "Review changes and report evidence-backed findings."
```

Claude Code uses `name` and `description` in frontmatter, with the prompt as the
Markdown body. Cursor and Gemini CLI also document that structure:

```markdown
---
name: reviewer
description: Reviews changes for correctness.
---

Review changes and report evidence-backed findings.
```

OpenCode derives the agent name from the filename. The adapter must set
`mode: subagent`; otherwise the generated definition may have a broader role:

```markdown
---
description: Reviews changes for correctness.
mode: subagent
---

Review changes and report evidence-backed findings.
```

These examples show structural mappings only. They do not restrict tools or
enforce read-only access.

## Capability differences

An absent field below means no equivalent was established by the cited agent
documentation. It does not prove that the product lacks the capability.

| ID | Capability | Codex | Claude Code | Cursor | Gemini CLI | OpenCode |
| --- | --- | --- | --- | --- | --- | --- |
| 6 | Model selection | Vendor-specific model ID | Alias or supported ID | `inherit` or supported ID | Vendor-specific model ID | `provider/model-id` |
| 7 | Reasoning and generation | Reasoning effort and other documented model settings | `effort`, subject to version | No generic effort field established | Temperature, turn limit, timeout | Provider-specific options |
| 8 | Tool filtering | Supported feature disables; no universal tool-name mapping | `tools`, `disallowedTools` | Inherits parent tools | `tools`; omission inherits parent | `permission`; legacy `tools` deprecated |
| 9 | Filesystem and approvals | Inherits parent sandbox and approvals | `permissionMode`, tool controls; parent can override | `readonly` | Separate policy controls; tool selection alone is insufficient | Fine-grained `allow` / `ask` / `deny` permissions |
| 10 | MCP access | Inherits parent servers | Agent `mcpServers` and tool controls | Parent MCP locally; team MCP in Cloud | Inline `mcpServers` | Tool permissions; per-agent server definitions not established |
| 11 | Skills | Inherits skills; documented exclusions and bundled-skill controls | `skills` preloads content, not exclusive access | Agent selector not established | Agent selector not established | `skill` tool permission, not a skill preload list |
| 12 | Delegation | Description supports role selection | Automatic and explicit delegation; `Agent` tool controls | Automatic delegation; `is_background` | Automatic routing and `@name` | Automatic routing and `@name`; `permission.task` gates agent calls |

Vendor behavior to account for in the mappings:

1. Current Codex documentation says agent files cannot replace the parent's
   sandbox or approval settings. `sandbox_mode`, `approval_policy`,
   `model_provider`, and `mcp_servers` in agent files do not override the parent.
   MCP server setup belongs in parent configuration.
2. Claude Code permission modes remain subject to parent state. Its `skills`
   field preloads skill content; it does not prevent discovery of other skills.
   Some frontmatter fields require particular versions, and unknown fields can
   be ignored.
3. Cursor documents subagents for Editor, CLI, and Cloud Agents. It also discovers
   Claude and Codex agent directories for compatibility. Generating all formats
   may expose duplicate roles; duplicate resolution and precedence were not
   established by this research.
4. Gemini CLI has an `experimental.enableAgents` setting that can disable agents.
   The current subagent page does not label the feature itself experimental.
   Its policy engine is separate from the Markdown agent definition.
5. OpenCode's `permission.task` restricts agent-to-agent calls. It does not prevent
   a user from invoking an agent with `@`. Provider options such as reasoning
   effort require a compatible provider and model.

## Simple generic subagent format

Use one Markdown file per agent. Define the name and description in YAML
frontmatter and the shared instructions in the body. An optional `vendors` section
holds settings that differ between vendors, using that vendor's vendor-specific setting names.

For example, `agents/reviewer.md`:

```markdown
---
name: reviewer
description: Reviews changes for correctness.
vendors:
  claude:
    model: sonnet
---

Review changes and report evidence-backed findings.
Return findings with file locations and unresolved questions.
```

For each vendor supplied to the command:

1. Read the shared name, description, and instructions.
2. Add settings from that vendor's optional section.
3. Write the vendor-specific agent file in its personal agent directory.

Codex receives TOML with the body as `developer_instructions`. Claude, Cursor, and
Gemini receive Markdown with vendor-specific frontmatter and the shared body. OpenCode
receives Markdown named after the agent, with `mode: subagent`. The generic
`vendors` wrapper is not emitted into any output file.

If a vendor section is absent, generate the shared role using that vendor's defaults.
Model identifiers and other vendor-specific settings are specified per vendor; the command
does not guess equivalent models or permissions.

### Command flow

```text
(1) Read source directory and vendors from command input
    -> (2) Create instruction and skill symlinks, skipping conflicting links
    -> (3) Read, validate, generate, and write each agent file independently
    -> (4) Warn and continue after an agent file fails
```

Generated agent files are refreshed on each run. Edit the source definitions to
make persistent changes. No ownership database, edit tracking, capability negotiation,
or vendor-version checks are part of this command.

### Validation and replacement

Read the generic subagents files from `<src_dir>/subagents/`. Only `.md` files in that
dir are read; other files and subdirs are skipped. A missing `subagents/`
directory is optional. The frontmatter must contain only `name`, `description`,
and optional `vendors`. Names start with a lowercase ASCII letter or digit and
contain only lowercase ASCII letters, digits, hyphens, and underscores. The name
determines the output filename; source filenames need not match. Duplicate names
and blank descriptions or bodies are errors. The body after the closing
frontmatter delimiter is preserved, including whitespace and subsequent `---` lines.

Each `vendors` key must name a supported vendor and contain a settings mapping.
Vendor-specific nested settings are retained only in that vendor's output. Settings cannot
replace `name`, `description`, `developer_instructions`, or the `vendors` wrapper.
`mode: subagent` is generated for OpenCode and cannot be overridden. Vendor-specific
settings, including Codex `skills.config`, sandbox settings, and MCP settings, are
preserved without a vendor-specific schema; the vendor validates their meaning.
Preserving a setting does not guarantee that the vendor supports it in a subagent file.

Instruction and skill links are handled before agent files. Each agent file is
validated and generated independently before writing its selected outputs. A read,
validation, generation, or write error produces an amber warning and does not stop
other agent files. Warnings identify the source path and, for generation errors,
the vendor. Duplicate names keep the first generated definition in filename order.
Generated regular files are overwritten when different, including manually edited files with matching output names.
Identical files are left untouched. Conflicting symlinks or directories at an
output filename, and symlinks or non-directories at an agent directory, produce
amber warnings and remain unchanged. Other agents and vendors continue syncing.
Unrelated files are left alone; deleting a source definition does not remove old
generated files.

## Remaining research and focused tests

Verify whether each vendor loads instruction and skill symlinks at runtime.
Agent files are generated directly. Check Cursor's discovery when
Claude and Codex agent definitions coexist.

Focused tests cover agent file generation for all five vendors, vendor settings,
and generated-file
replacement and no-ops, independent handling of invalid or failing agent files,
and warning-and-continue behavior for conflicting outputs. Vendor runtime loading remains unverified.

## Official sources and research limits

| ID | Source | Evidence used |
| --- | --- | --- |
| 1 | [Codex subagents][codex] | Standalone TOML, inheritance, feature and skill controls |
| 2 | [Claude Code subagents][claude] | Frontmatter, discovery, tools, permissions, MCP, skill preload |
| 3 | [Cursor subagents][cursor] | Product surfaces, compatibility discovery, model, readonly, background |
| 4 | [Gemini CLI subagents][gemini] | Frontmatter, discovery, tools, MCP, execution limits, feature toggle |
| 5 | [OpenCode agents][opencode] | Markdown and JSON formats, modes, permissions, provider options |

Documentation research was delegated to one agent and synthesized here. Findings
describe fetched official pages, not live enforcement tests. Undocumented behavior
remains unverified. This research did not establish instruction or skill discovery
through symlinks. Agent symlinks are not part of the command.

[codex]: https://learn.chatgpt.com/docs/agent-configuration/subagents.md
[claude]: https://code.claude.com/docs/en/sub-agents.md
[cursor]: https://cursor.com/docs/subagents
[gemini]: https://geminicli.com/docs/core/subagents.md
[opencode]: https://opencode.ai/docs/agents.md
