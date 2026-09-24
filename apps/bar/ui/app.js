// The panel: it renders what the watcher sends and asks the app to
// open things. It computes nothing about a repository itself — that
// is den-core's job, once, for both surfaces.

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const el = {
  list: document.getElementById('list'),
  meta: document.getElementById('meta'),
  ciAge: document.getElementById('ci-age'),
  refresh: document.getElementById('refresh'),
  add: document.getElementById('add'),
  quit: document.getElementById('quit'),
  update: document.getElementById('update'),
};

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

function age(secs) {
  if (secs == null) return '';
  if (secs < 90) return 'just now';
  if (secs < 5400) return `${Math.round(secs / 60)}m ago`;
  if (secs < 172800) return `${Math.round(secs / 3600)}h ago`;
  return `${Math.round(secs / 86400)}d ago`;
}

function counts(repo) {
  const out = [];
  if (repo.conflicted) out.push([`!${repo.conflicted}`, 'c-conflict']);
  if (repo.staged) out.push([`+${repo.staged}`, 'c-dirty']);
  if (repo.modified) out.push([`~${repo.modified}`, 'c-dirty']);
  if (repo.untracked) out.push([`?${repo.untracked}`, 'c-dirty']);
  if (repo.stashed) out.push([`⚑${repo.stashed}`, 'c-dirty']);
  if (repo.ahead) out.push([`↑${repo.ahead}`, 'c-ahead']);
  if (repo.behind) out.push([`↓${repo.behind}`, 'c-behind']);
  if (repo.ci) {
    const mark = { success: '✓', failure: '✗', running: '•', unknown: '?' }[repo.ci.state] || '?';
    out.push([mark, `c-ci-${repo.ci.state}`]);
  }
  return out;
}

function subtitle(repo) {
  if (repo.error) return repo.error;
  if (repo.uninitialized) return 'no commits yet';
  const bits = [];
  if (repo.branch) bits.push(repo.branch);
  if (repo.tag) {
    bits.push(repo.commits_since_tag ? `${repo.tag} +${repo.commits_since_tag}` : repo.tag);
  }
  if (repo.ci && repo.ci.state === 'failure' && repo.ci.failed_step) {
    bits.push(repo.ci.failed_step);
  }
  return bits.join(' · ');
}

function repoRow(repo) {
  const row = document.createElement('div');
  row.className = 'row';
  row.tabIndex = 0;
  row.dataset.state = repo.state;
  if (repo.ci) row.dataset.ci = repo.ci.state;

  const dot = document.createElement('span');
  dot.className = 'dot';

  const body = document.createElement('div');
  body.className = 'body';
  const title = document.createElement('div');
  title.className = 'name';
  title.textContent = repo.name;
  if (repo.pinned) {
    const pin = document.createElement('span');
    pin.className = 'pin';
    pin.textContent = ' ★';
    title.append(pin);
  }
  const sub = document.createElement('div');
  sub.className = 'sub';
  sub.textContent = subtitle(repo);
  body.append(title, sub);

  const tally = document.createElement('div');
  tally.className = 'counts';
  for (const [text, cls] of counts(repo)) {
    const span = document.createElement('span');
    span.className = cls;
    span.textContent = text;
    tally.append(span);
  }
  // Plain click opens a shell there; the alternates follow the
  // convention the rest of macOS uses for "somewhere else".
  const activate = (event) => {
    if (event.metaKey) {
      invoke('open', { path: repo.path, how: 'github' });
    } else if (event.altKey) {
      invoke('open', { path: repo.path, how: 'editor' });
    } else if (event.shiftKey) {
      invoke('open', { path: repo.path, how: 'finder' });
    } else if (repo.ci && repo.ci.state === 'failure' && repo.ci.url) {
      invoke('open_url', { url: repo.ci.url });
    } else {
      invoke('open', { path: repo.path, how: 'terminal' });
    }
    invoke('hide_panel');
  };
  row.addEventListener('click', activate);
  row.addEventListener('keydown', (event) => {
    if (event.key === 'Enter') activate(event);
  });

  row.append(dot, body, tally);
  return row;
}

function folderBlock(folder) {
  const wrap = document.createDocumentFragment();

  const head = document.createElement('div');
  head.className = 'section';
  const name = document.createElement('span');
  name.className = 'section-name';
  name.textContent = folder.label;
  name.title = folder.path;
  const roll = document.createElement('span');
  roll.className = 'section-roll';
  roll.textContent = folder.missing
    ? 'folder is gone'
    : [
        plural(folder.total, 'repo', 'repos'),
        folder.dirty ? `${folder.dirty} dirty` : null,
        folder.failing ? `${folder.failing} CI failing` : null,
      ]
        .filter(Boolean)
        .join(' · ');
  const drop = document.createElement('button');
  drop.className = 'section-drop';
  drop.type = 'button';
  drop.textContent = '✕';
  drop.title = 'Stop watching this folder';
  drop.addEventListener('click', () => invoke('remove_folder', { path: folder.path }));
  head.append(name, roll, drop);
  wrap.append(head);

  if (folder.missing) {
    const note = document.createElement('div');
    note.className = 'item item-quiet';
    note.textContent = folder.path;
    wrap.append(note);
  } else if (!folder.repos.length) {
    const note = document.createElement('div');
    note.className = 'item item-quiet';
    note.textContent = 'No repositories here yet.';
    wrap.append(note);
  }

  for (const repo of folder.repos) wrap.append(repoRow(repo));
  return wrap;
}

function render(snapshot) {
  el.list.replaceChildren();

  if (!snapshot.folders.length) {
    const empty = document.createElement('div');
    empty.className = 'item item-quiet';
    empty.textContent = 'No folders watched yet.';
    el.list.append(empty);
  } else {
    snapshot.folders.forEach((folder, i) => {
      if (i) el.list.append(document.createElement('hr'));
      el.list.append(folderBlock(folder));
    });
  }

  // The sections carry the counts, so the footer carries the thing
  // nothing else can say: what the modifiers do.
  el.meta.textContent = snapshot.scanning ? 'scanning…' : '⌘ GitHub · ⌥ editor · ⇧ Finder';

  // The item carries its own state: an invitation to ask, the asking,
  // the answer, or the update itself.
  el.update.classList.toggle('is-offer', Boolean(snapshot.update));
  el.update.textContent = snapshot.update
    ? `Update to ${snapshot.update}`
    : snapshot.checking
      ? 'Checking…'
      : snapshot.checked
        ? `den ${snapshot.version} · up to date`
        : `den ${snapshot.version} · check for updates…`;

  el.ciAge.textContent = snapshot.gh
    ? snapshot.ci_age_secs == null
      ? 'CI not checked yet'
      : `CI ${age(snapshot.ci_age_secs)}`
    : 'gh not signed in';
  el.meta.title = 'A row opens a shell there';
}

el.refresh.addEventListener('click', () => invoke('rescan'));
el.add.addEventListener('click', () => invoke('add_folder'));
el.quit.addEventListener('click', () => invoke('quit'));
el.update.addEventListener('click', () => {
  if (el.update.classList.contains('is-offer')) {
    el.update.textContent = 'Updating…';
    invoke('install_update');
  } else {
    invoke('check_updates');
  }
});
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') invoke('hide_panel');
  if (event.key === 'r' && (event.metaKey || event.ctrlKey)) invoke('rescan');
});

listen('den://snapshot', (event) => render(event.payload));
invoke('snapshot').then(render);
