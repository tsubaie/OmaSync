// The page's layering: building blocks (ui, format, theme) know nothing about app state; only
// api.ts and the state hook talk to Tauri; panes don't reach into each other.
const rule = (name, comment, from, to) => ({ name, comment, severity: 'error', from, to });

module.exports = {
  forbidden: [
    rule('no-cycles', 'Import cycles make load order and testing fragile.', {}, { circular: true }),
    rule('no-unresolved', 'Every import must resolve.', {}, { couldNotResolve: true }),
    rule(
      'only-api-and-state-talk-to-tauri',
      'Calls into Rust go through src/api.ts so their types live in one place.',
      { path: '^src/', pathNot: '^src/(?:api|state|mocks)\\.ts$' },
      { path: '@tauri-apps/api' },
    ),
    rule(
      'building-blocks-stay-generic',
      'ui, format and theme are reusable; they must not depend on app state or panes.',
      { path: '^src/(?:ui\\.tsx|format\\.ts|theme\\.ts)$' },
      { path: '^src/(?:state\\.ts|App\\.tsx|panes/)' },
    ),
    rule(
      'panes-are-independent',
      'A pane may share NoticeLine but not import another pane.',
      { path: '^src/panes/([^/]+)\\.tsx$' },
      { path: '^src/panes/', pathNot: ['^src/panes/$1\\.tsx$', '^src/panes/NoticeLine\\.tsx$'] },
    ),
  ],
  options: {
    // TypeScript 7 has no compiler API yet; swc parses the sources instead (as in enan).
    parser: 'swc',
    doNotFollow: { path: 'node_modules' },
    tsConfig: { fileName: 'tsconfig.json' },
    exclude: { path: '\\.test\\.ts$' },
  },
};
