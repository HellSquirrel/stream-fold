# js-framework-benchmark, official driver, this machine

Raw result files written by `webdriver-ts` (medians of 15 or 25 runs,
Chrome tracing, paint included), 2026-09-17, headless Chrome from
Puppeteer, the development machine. Three frameworks were run in one
session: `keyed/logfold`, `keyed/vanillajs`, `keyed/react-hooks`.

`glue/` is the LogFold entry, verbatim: the reference page, a
`package.json` for the driver, and `src/glue.mjs`, which applies the
component's patches to the reference markup and composes the label text
from the numbers. It stands in for `www/logfold.mjs` on that page only.
It is not part of the framework and nothing in the framework was changed
for it; it exists so the numbers can be reproduced. `dist/` is the
`bench-app` bundle plus `www/gen/bench.manifest.mjs`, copied from a build.

To rerun: clone the benchmark, `npm ci`, `npm run install-server`,
`npm run install-webdriver-ts`; copy `glue/` to
`frameworks/keyed/logfold/` and the built bundle into its `dist/`;
`npm install --package-lock-only` there (the server requires a lockfile);
`npm start`; then from `webdriver-ts`:
`npm run isKeyed -- keyed/logfold` and
`npm run bench -- --framework keyed/logfold keyed/vanillajs --headless true`.
