# Reef docs

Start with [architecture.md](architecture.md). It shows how a line of input travels through Reef and which file owns each step.

| Doc | Read it when you want to... |
|---|---|
| [architecture.md](architecture.md) | understand how the pieces fit together |
| [lexer.md](lexer.md) | know exactly how Reef splits a line into words and handles quotes |
| [builtins.md](builtins.md) | add a new builtin command |
| [io.md](io.md) | understand why builtins never print directly, and how redirects will plug in |
| [platform.md](platform.md) | add or change anything that differs between Windows and Linux |
| [testing.md](testing.md) | run the tests or add new ones |
| [design/](design/) | see the big decisions and why they were made |

## Rules for these docs

- They describe what exists. Anything not built yet is marked **planned**.
- Decisions ("why X") go in `design/` as numbered records. Topic docs explain how things work now.
- When a change alters how something works, update its doc in the same change.
