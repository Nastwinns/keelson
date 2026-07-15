# Agents

Token-saving output rules for any AI coding agent working in this repo (Claude Code, Codex,
Cursor, Windsurf, Copilot, Amp, Devin — all read `AGENTS.md` natively). Sourced from
[espresso](https://github.com/mirkobozzetto/espresso) (MIT). 40–60% fewer output tokens.

## Output Rules

- Max 2 sentences per paragraph. Line break after every full stop.
- Max 120 chars per line — split if longer.
- Bullet points by default unless user asks otherwise.
- Fragments OK. Short synonyms preferred.

## Forbidden Patterns

Openers: "I will", "Sure", "Here is", "Of course", "Perfect", "Great", "Certainly", "I'd be happy to", "Absolutely", "Let me"
Closers: recap, "In summary", "Let me know if", "Hope this helps", "Feel free to ask"
Filler: just, really, basically, actually, simply, certainly, definitely, essentially, obviously, clearly, literally
Hedging: "I think" → state directly. "It seems like" → state directly. "might be" → is/isn't.

## Style

- Result first, not narration. Lead with the answer.
- One proposal by default. Multiple only if user asks.
- No comments in code. No emojis unless asked.
- No recap at end of response.
- Direct corrections over apology loops.

## Project specifics

- All business logic in `haw-core`; CLI and TUI stay thin. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- `unsafe_code = forbid`, `unwrap_used`/`expect_used = warn` workspace-wide — respect them.
- Determinism is a hard requirement (cert evidence): no wall-clock, no unordered iteration in
  serialization. See [docs/COMPLIANCE.md](docs/COMPLIANCE.md) §8.
