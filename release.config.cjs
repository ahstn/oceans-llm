const changelogTitle = `# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).`;

const releaseRules = [
  { breaking: true, release: "major" },
  { revert: true, release: false },
  { type: "feat", release: "minor" },
  { type: "fix", release: "patch" },
  { type: "perf", release: false },
  { type: "docs", release: false },
  { type: "style", release: false },
  { type: "refactor", release: false },
  { type: "test", release: false },
  { type: "build", release: false },
  { type: "ci", release: false },
  { type: "chore", release: false },
];

const parserOpts = {
  noteKeywords: ["BREAKING CHANGE", "BREAKING CHANGES"],
};

const releaseBodyTemplate = String.raw`<% if (env.GATEWAY_DIGEST && env.ADMIN_UI_DIGEST && env.OWNER_LC) { %>## Container Images

- \`<%= env.REGISTRY %>/<%= env.OWNER_LC %>/<%= env.GATEWAY_IMAGE_NAME %>:<%= nextRelease.gitTag %>\` - digest \`<%= env.GATEWAY_DIGEST %>\`
- \`<%= env.REGISTRY %>/<%= env.OWNER_LC %>/<%= env.ADMIN_UI_IMAGE_NAME %>:<%= nextRelease.gitTag %>\` - digest \`<%= env.ADMIN_UI_DIGEST %>\`

<% } %><%= nextRelease.notes %>`;

module.exports = {
  branches: ["main"],
  tagFormat: "v${version}",
  plugins: [
    [
      "@semantic-release/commit-analyzer",
      {
        preset: "conventionalcommits",
        presetConfig: {},
        parserOpts,
        releaseRules,
      },
    ],
    [
      "@semantic-release/release-notes-generator",
      {
        preset: "conventionalcommits",
        presetConfig: {},
        parserOpts,
      },
    ],
    [
      "@semantic-release/changelog",
      {
        changelogFile: "CHANGELOG.md",
        changelogTitle,
      },
    ],
    [
      "@semantic-release/exec",
      {
        prepareCmd: 'node ./scripts/release-cargo-prepare.mjs "${nextRelease.version}"',
      },
    ],
    [
      "@semantic-release/git",
      {
        assets: ["CHANGELOG.md", "crates/*/Cargo.toml"],
        message: "docs(release): update changelog for ${nextRelease.gitTag} [skip ci]",
      },
    ],
    [
      "@semantic-release/github",
      {
        failComment: false,
        releasedLabels: false,
        releaseBodyTemplate,
        successComment: false,
      },
    ],
  ],
};
