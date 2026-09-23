# Documentation and wiki

The main Labello repository contains the editable documentation. GitHub Wiki is
a generated copy in `HULKs/labello.wiki.git`. Edit the repository files and review
them with the code they describe; direct wiki edits are replaced on publication.

## What lives where

| Location | Contents | Published |
| --- | --- | --- |
| `README.md` | Project overview, quick start, navigation, principal limitations | Overview |
| `docs/README.md` | Complete current-documentation index | Home |
| Current `docs/*.md` | User guides and technical references | Named wiki pages |
| `CONTRIBUTING.md`, inspector and guest READMEs | Contributor, inspection, and provisioning instructions | Named wiki pages |
| `docs/archive/` | All plans, feature drafts, target specification, historical records | Never |
| `AGENTS.md`, `.agents/skills/` | Repository agent instructions | Never |
| `docs/wiki.json` | Explicit source/page list and sidebar groups | Drives publication |

The publisher rejects archive sources, duplicate page names, missing current
pages, and unsafe output paths. Only listed files become pages. Links between
published documents become wiki links; code, assets, agent guidance, and archive
links point back to the main repository. Code examples remain unchanged.

There are no document owner, status, audience, date, or revision headers. Git
records that history. Code ownership tables, event timestamps, build identity,
and version-specific compatibility rules are substantive documentation and remain.

## Check and preview

Python 3.10 or newer and Git are sufficient; no Python packages are required.
From the repository root:

```sh
python3 scripts/docs.py check
python3 -m unittest discover -s scripts -p 'test_docs.py'
python3 scripts/docs.py build /tmp/labello-wiki-preview
./scripts/verify.sh changed origin/main
```

The check covers tracked and new Markdown, local files and heading anchors,
metadata headers, and wiki page coverage. It skips links inside code examples and
does not fetch external URLs. Archived relative links are checked too; historical
source paths written as code remain historical observations.

Preview output contains the pages, `_Sidebar.md`, `_Footer.md`, and a generated-page
inventory. Rebuilding removes only obsolete pages in that inventory and preserves
unrelated files. Use a disposable directory or the wiki checkout, never a source
folder. Keep generated pages out of the main repository.

## Automatic publication

The [wiki workflow](../.github/workflows/wiki.yml) checks pull requests and publishes
current documentation after relevant changes reach `main`. It can also be run
manually from `main`. Pull-request jobs have read-only permissions and never receive
the publishing credential. The canonical verification command checks the same
source and link rules locally.

One-time repository setup:

1. Enable Wiki and create its first page in GitHub. GitHub requires an initial
   page before its [wiki Git repository can be cloned](https://docs.github.com/en/communities/documenting-your-project-with-wikis/adding-or-editing-wiki-pages).
2. Add an Actions secret named `WIKI_TOKEN` with Git write access to this wiki.
   Use a dedicated publishing credential approved for HULKs. For a classic token,
   use `public_repo` for this public repository, with any required organization
   authorization. Keep the value out of Markdown, URLs, and command arguments.
3. Merge the documentation and workflow through the normal review process, then
   check the Documentation wiki workflow and the published Home/sidebar links.

Authentication uses Git's credential helper. Publication makes an ordinary commit
and push to the wiki's existing default branch. It does not force-push or rewrite
history. A missing credential or concurrent non-fast-forward update fails visibly;
resolve the competing edit and rerun. There is no successful silent skip.

## Publish manually

With existing GitHub SSH access, clone outside the main checkout:

```sh
git clone git@github.com:HULKs/labello.wiki.git /tmp/labello-wiki
python3 scripts/docs.py build /tmp/labello-wiki
git -C /tmp/labello-wiki diff --check
git -C /tmp/labello-wiki diff --stat
git -C /tmp/labello-wiki add --all
git -C /tmp/labello-wiki commit -m 'Update current Labello documentation'
git -C /tmp/labello-wiki push origin HEAD
```

Review generated changes before committing. Reuse a clean checkout after fetching
and fast-forwarding it; omit the commit/push when output is unchanged. Repository
links target the configured source ref, normally `main`, so new source-file links
become available when the corresponding repository change is published.

## Add or move documentation

Keep current behavior in a focused guide or reference and link it from Home.
Add its source and stable page name to `docs/wiki.json`. Use ordinary relative
Markdown links so navigation works in both the checkout and wiki. Keep headings
stable where practical; update anchors when changing them. Use inline links or
reference definitions, and keep code examples in fenced blocks.

Put non-current material anywhere beneath `docs/archive/` and link it from its
index. Preserve its technical content without treating it as current guidance.
If an old plan contains a still-current contract, maintain that contract as a
current reference, as with [workflow policy](workflow-policy.md).
