module.exports = {
  preset: '@react-native/jest-preset',
  // pnpm nests real package files under node_modules/.pnpm/<pkg>/node_modules/<pkg>,
  // so the preset's default transformIgnorePatterns (which only special-cases a bare
  // `node_modules/react-native/...`) never matches and RN's ESM sources go untransformed.
  // Allow-list `.pnpm` itself so the pattern re-checks the nested react-native segment.
  transformIgnorePatterns: [
    'node_modules/(?!(\\.pnpm|(jest-)?react-native|@react-native(-community)?)/)',
  ],
};
