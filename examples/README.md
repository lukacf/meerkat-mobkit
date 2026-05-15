# Examples

This repo now has a repo-root `examples/` surface for multi-language packs that prove the shipped MobKit surfaces end to end.

Numbered packs are intentionally browser-first and can include:

- a stock Rust example server
- a browser smoke proof
- TypeScript and Python helpers
- a shared scenario file
- operator drill prompts

## Packs

- `001-incident-command-center-pack`
- `002-foresight-studio-pack`

Run the first pack with:

```bash
cd examples && npm install
./001-incident-command-center-pack/examples.sh
```

Run the second pack's offline structure check with:

```bash
./002-foresight-studio-pack/examples.sh --smoke
```

Run the live customized console with:

```bash
export OPENAI_API_KEY=...
./002-foresight-studio-pack/examples.sh --kickoff
```
