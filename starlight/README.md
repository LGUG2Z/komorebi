# starlight

The documentation site of komorebi, for Windows and macOS, built with [Starlight](https://starlight.astro.build).

```sh
npm ci
npm run dev      # the site on localhost, after generating the komorebic pages
npm run build    # dist/, which `wrangler.jsonc` deploys
```

The guides are written by hand in `src/content/docs/`. The reference pages (`src/content/docs/reference/` and
`src/data/cli/`) are generated: run `just docgen` from the root of the repository on Windows or macOS after a change
of a command or of a schema, and commit what it writes. It writes the page of the platform it runs on, so the
pages of the other one stay as they are. `komorebic-<platform>.mdx` is made by `scripts/generate-cli-reference.cjs`
from the pages of the commands and is not committed (`npm run dev` and `npm run build` make it too).
