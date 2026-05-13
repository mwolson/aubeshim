/** @type {import("prettier").Config} */
export default {
  overrides: [
    {
      files: "*.md",
      options: {
        endOfLine: "lf",
        printWidth: 80,
        proseWrap: "always",
        tabWidth: 2,
        useTabs: false,
      },
    },
  ],
};
