# @go-vernier/cli

**Which services can this change reach?** Vernier reads a repository, finds
its services, maps which service calls which, and shows the blast radius of a
pull request before you merge it.

```bash
npx @go-vernier/cli                  # what your changes on this branch can reach
npx @go-vernier/cli --pr 481         # what one pull request can reach
npx @go-vernier/cli --explain        # plus a plain-English summary from your own LLM key
npx @go-vernier/cli tui              # explore it interactively
```

Or install it: `npm install -g @go-vernier/cli`, then run `vernier`.

This package runs a prebuilt binary for macOS (Apple silicon, Intel), Linux
(x64, arm64) and Windows (x64), installed as an optional dependency. Nothing
runs at install time, and nothing is downloaded at run time.

Documentation, other ways to install, and the source:
<https://github.com/Go-Vernier/Vernier-OSS>
