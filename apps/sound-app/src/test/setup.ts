import "@testing-library/jest-dom/vitest"

// jsdom does not implement window.matchMedia. next-themes calls it during
// mount to detect the system color scheme, so tests that render
// ThemeProvider need this polyfill.
Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  }),
})
