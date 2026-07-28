// @ts-check
// ESLint flat config for the Angular frontend. Type-aware: typescript-eslint
// recommendedTypeChecked + stylisticTypeChecked (parserOptions.projectService)
// for usage bugs tsc/syntactic-lint miss (floating/misused promises, unsafe
// `any`, await-thenable), plus the Angular rules (forbid inline template:/styles:
// — the team's angular-external-template-style rule — and template a11y).
// It's fast so it runs as the normal lint in CI; `npm run lint`.

import angular from "angular-eslint";
import tseslint from "typescript-eslint";

export default tseslint.config(
  // ts-rs writes src/app/generated/ from the Rust types — don't lint generated code.
  { ignores: ["src/app/generated/**"] },
  {
    files: ["src/**/*.ts"],
    extends: [
      ...tseslint.configs.recommendedTypeChecked,
      ...tseslint.configs.stylisticTypeChecked,
      ...angular.configs.tsRecommended,
    ],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
    },
    processor: angular.processInlineTemplates,
    rules: {
      "@angular-eslint/component-max-inline-declarations": ["error", { template: 0, styles: 0 }],
      // `x as Shape` is a claim, not a check — and it is the one hole in the
      // otherwise-total protection against "[object Object]" reaching the
      // screen. dev-lint's DL-ANGULAR-STRINGIFIED-OBJECT types every template
      // expression honestly, so it can only be fooled by a type we manufactured
      // ourselves: the log sheet asserted an HTTP error body into
      // `{ error?: { error?: string } }` and rendered the result, and no layer
      // could see it. Narrow at the boundary instead — src/app/shared/narrow.ts.
      "@typescript-eslint/no-unsafe-type-assertion": "error",
      "@typescript-eslint/no-empty-function": "off",
    },
  },
  {
    files: ["src/**/*.html"],
    extends: [...angular.configs.templateRecommended, ...angular.configs.templateAccessibility],
  },
);
