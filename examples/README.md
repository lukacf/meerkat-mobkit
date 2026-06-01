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
- `003-swarm-stress-pack`
- `004-mdm-console-pack`

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

Run the third pack's browser-driven real Gemini stress smoke with a 300-agent baseline plus a 240-agent burst:

```bash
./003-swarm-stress-pack/examples.sh --browser-smoke
```

Run the MDM console pack's local target smoke:

```bash
npm run mdm:smoke
npm run mdm:auth-smoke
npm run mdm:browser-smoke
npm run mdm:docker-smoke
```
