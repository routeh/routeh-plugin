# Releasing a plugin

A version goes from a plugin's repository to a feed the router reads
([BUNDLE.md](BUNDLE.md), The feed). Each plugin chooses one of two ways
(`publish_to` of `plugin.yml`):

| | the managed feed (`feed`) | its own bucket (`s3`) |
|---|---|---|
| who signs | the feed's repository | the plugin, with its own key |
| who writes the index | the feed's workflow only | the plugin only |
| what is shown | the feed decides (`feed.toml`) | what the plugin published |
| served by | GitHub Pages | any S3 |

The router does not know which way a feed was made.

A release starts from a tag `v<version>` (every target), the button
(a version, a release, some targets), or the daily schedule (the latest
tag, for the targets the feed has no build of: a new kernel). Building
and the QEMU test are the same either way ([BUILD.md](BUILD.md)); the
bundles come out unsigned.

## The managed feed

```
plugin's repository (plugin.yml)          feed's repository (feed.yml)
  unsigned bundles
  push --force to branch plugin/<id>  ──▶  incoming (the branch's own workflow)
                                             └▶ workflow_run, on main:
                                                new bundles signed → blobs/
                                                index.json rebuilt, signed
                                                commit to main, deploy Pages
                                            also: daily, feed.toml changed
```

- **A plugin proposes.** Its branch `plugin/<id>` holds the bundles of
  its last release in one commit with no parent, pushed with `--force`;
  it does not grow. The plugin holds a deploy key and nothing else: it
  knows nothing of the index, the signature or other plugins.
- **The feed decides.** Its workflow is the only writer of `main`
  (a ruleset) and the only holder of the signing key (the environment
  `feed`, deployments from `main` only). One run at a time:
  1. the bundles of every `plugin/*` branch are read; one whose id is not
     its branch's, or that is no bundle, is refused and the others go
     on (the run fails at its end);
  2. those not held yet are signed into `blobs/<sha256>`; the same
     version built again replaces its blob;
  3. `index.json` is rebuilt from every blob and `feed.toml`, signed,
     committed and deployed.
- **No race.** The index is derived from what the feed holds, never
  edited, so the last run is right. The daily run signs it again with a
  new `expires`.
- **Why `incoming`.** A push runs only the workflows of the commit
  pushed, and a plugin's branch has none of the feed's; so it carries one
  that does nothing, and the feed's own workflow runs on `main` after it.
  A workflow of a branch never sees the key.

```toml
# feed.toml
[awg]
keep = 3                       # newest versions shown per architecture and kernel
yank = ["3.1.20261113.2-r1"]   # not shown; its blob stays
```

`main`'s history is the log of releases: every blob ever published, a
commit for each change of the index.

## A plugin's own bucket

```
plugin's repository (plugin.yml, publish_to: s3)
  index.json and its signature from s3://<bucket>/<prefix>/
  bundles signed with the plugin's own key → blobs/
  their builds added to the index, signed
  uploaded: the blobs, then index.json, then its signature
```

- **Its own key**, which the router trusts for that feed only.
- **One plugin a prefix**: the plugin is its feed's only writer, so the
  index is added to rather than rebuilt, and a run needs only the index
  (`cargo xtask publish`). The same version built again replaces its
  build; the old blob stays, named by nothing.
- The daily run signs the index again when there is nothing to build.
- Blobs are cached for a year (`immutable`), the index not at all.
- Any S3: AWS, or another by `s3_endpoint` (Google Cloud Storage,
  MinIO). Who may read the bucket is the bucket's business.

## Setting it up

The feed's repository (`templates/feed/`):

- Pages built by GitHub Actions;
- the environment `feed`, deployments from `main` only, with the secret
  `FEED_SIGNING_KEY` (`routeh-plugin keygen`); routers trust its public
  key;
- a ruleset on `main`: no deletion, no force push, updates only by
  GitHub Actions and the admins;
- a deploy key with write access, whose secret half the plugins have.

A plugin's repository (`templates/plugin/`), for the managed feed:

- the variables `FEED_REPOSITORY` and `FEED_URL`, the secret
  `FEED_DEPLOY_KEY` (one for all plugins, as an organisation secret: a
  ruleset cannot tell deploy keys apart anyway);

or for its own bucket:

- `publish_to: s3`, the bucket's variables, the secrets `S3_SIGNING_KEY`,
  `S3_ACCESS_KEY_ID` and `S3_SECRET_ACCESS_KEY` (a key that may write its
  prefix only).

If routeh-plugin is private, its reusable workflows must allow the
organisation's repositories, and a plugin's xtask needs a token for its
git dependency.

A bundle in the feed's git is hundreds of KB to a few MB; if `blobs/`
grows heavy it moves to the feed's Releases and nothing else changes.
