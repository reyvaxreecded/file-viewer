// Native File/Blob networking: file payloads never cross WebAssembly memory.
const jobs = new Map();
let running = 0, installed = false, dirty = false, collisionChoice = null;
const MAX_RUNNING = 2, CHUNK = 4 * 1024 * 1024;
const $ = (id) => document.getElementById(id);
const notify = (path) => window.dispatchEvent(new CustomEvent('glassftp-change', { detail: path }));
const parent = (path) => path.slice(0, path.lastIndexOf('/')) || '/';
const join = (path, name) => `${path === '/' ? '' : path}/${name}`;
const human = (n) => n >= 1073741824 ? `${(n / 1073741824).toFixed(2)} Go` : n >= 1048576 ? `${(n / 1048576).toFixed(1)} Mo` : n >= 1024 ? `${(n / 1024).toFixed(1)} Ko` : `${n} o`;
async function request(path, { method = 'GET', data, body, signal, headers = {} } = {}) {
  if (method !== 'GET') headers['X-GlassFTP'] = '1';
  if (data !== undefined) { headers['Content-Type'] = 'application/json'; body = JSON.stringify(data); }
  const response = await fetch(`/api/v1${path}`, { method, body, signal, headers, credentials: 'same-origin' });
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}));
    const error = new Error(payload.message || `Requête refusée (${response.status})`);
    error.status = response.status; throw error;
  }
  return response.status === 204 ? null : response.json();
}
export function setDirty(value) { dirty = value; }
export function pickFiles(path, folder) {
  const input = document.createElement('input'); input.type = 'file'; input.multiple = true;
  if (folder) input.webkitdirectory = true;
  input.addEventListener('change', async () => {
    collisionChoice = null;
    for (const file of input.files) {
      const relative = file.webkitRelativePath || file.name;
      try { await ensureParents(path, relative); enqueue(file, join(path, relative)); }
      catch (e) { showFailure(file.name, e.message); }
    }
  });
  input.click();
}
async function ensureParents(base, relative) {
  const parts = relative.split('/'); parts.pop(); let path = base;
  for (const part of parts) {
    path = join(path, part);
    try { await request('/folders', { method: 'POST', data: { path } }); }
    catch (e) { if (e.status !== 409) throw e; const info = await request(`/files/info?path=${encodeURIComponent(path)}`); if (info.kind !== 'directory') throw e; }
  }
}
function enqueue(file, path) {
  const localId = crypto.randomUUID();
  jobs.set(localId, { localId, file, path, size: file.size, offset: 0, state: 'queued', message: 'En attente', started: 0 });
  render(); pump();
}
function showFailure(name, message) {
  const localId = crypto.randomUUID(); jobs.set(localId, { localId, path: name, size: 0, offset: 0, state: 'error', message }); render();
}
function pump() {
  for (const job of jobs.values()) {
    if (running >= MAX_RUNNING) break;
    if (job.state !== 'queued') continue;
    running++; job.state = 'running';
    run(job).finally(() => { running--; render(); pump(); });
  }
}
async function run(job) {
  job.controller = new AbortController(); job.started = performance.now(); job.initialOffset = job.offset;
  try {
    if (!job.id) {
      try { const u = await request('/uploads', { method: 'POST', data: { path: job.path, size: job.size }, signal: job.controller.signal }); job.id = u.id; }
      catch (e) {
        if (e.status !== 409) throw e;
        const choice = collisionChoice || await conflict(job.path);
        if (choice === 'skip') { job.state = 'skipped'; job.message = 'Ignoré — fichier existant'; return; }
        if (choice === 'cancel') { job.state = 'cancelled'; job.message = 'Annulé'; return; }
        const name = job.file.name, dot = name.lastIndexOf('.');
        const stem = dot > 0 ? name.slice(0, dot) : name, ext = dot > 0 ? name.slice(dot) : '';
        for (let n = 2; n < 1000; n++) {
          const candidate = join(parent(job.path), `${stem} (${n})${ext}`);
          try { const u = await request('/uploads', { method: 'POST', data: { path: candidate, size: job.size }, signal: job.controller.signal }); job.path = candidate; job.id = u.id; break; }
          catch (e) { if (e.status !== 409) throw e; }
        }
        if (!job.id) throw new Error('Impossible de trouver un nom disponible');
      }
    }
    // Server offset is authoritative after a lost acknowledgment.
    const status = await request(`/uploads/${job.id}`); job.offset = status.offset;
    if (status.status === 'complete') { job.state = 'complete'; job.message = 'Terminé'; return; }
    if (status.status !== 'active') throw new Error('Finalisation à vérifier côté serveur. Le journal est conservé.');
    while (job.offset < job.size) {
      if (job.state !== 'running') return;
      const end = Math.min(job.offset + CHUNK, job.size);
      job.message = 'Envoi en cours'; render();
      const result = await request(`/uploads/${job.id}`, { method: 'PATCH', body: job.file.slice(job.offset, end), signal: job.controller.signal, headers: { 'Upload-Offset': String(job.offset), 'Content-Type': 'application/octet-stream' } });
      job.offset = result.offset; render();
    }
    job.message = 'Finalisation SFTP…'; render();
    const done = await request(`/uploads/${job.id}/complete`, { method: 'POST', data: {} });
    if (done.status !== 'complete') throw new Error('Finalisation non confirmée');
    job.state = 'complete'; job.message = 'Terminé'; job.file = null; notify(parent(job.path));
  } catch (error) {
    if (job.state === 'paused' || job.state === 'cancelled') return;
    job.state = 'error'; job.message = error.message;
  }
}
let dialogQueue = Promise.resolve();
function conflict(path) {
  const pending = dialogQueue.then(() => collisionChoice || new Promise((resolve) => {
    const dialog = document.createElement('dialog'); dialog.className = 'dialog conflict-dialog';
    const title = document.createElement('h2'); title.textContent = 'Ce nom existe déjà';
    const description = document.createElement('p'); description.textContent = `${path} — aucun fichier ne sera écrasé.`;
    const label = document.createElement('label'), check = document.createElement('input'); check.type = 'checkbox'; label.append(check, ' Appliquer aux prochains conflits');
    const actions = document.createElement('div'); actions.className = 'dialog-actions';
    for (const [value, name] of [['cancel', 'Annuler'], ['skip', 'Ignorer'], ['both', 'Conserver les deux']]) {
      const button = document.createElement('button'); button.textContent = name; button.className = value === 'both' ? 'primary' : 'secondary';
      button.onclick = () => { if (check.checked) collisionChoice = value; dialog.close(); dialog.remove(); resolve(value); }; actions.append(button);
    }
    dialog.addEventListener('cancel', (e) => { e.preventDefault(); dialog.remove(); resolve('cancel'); });
    dialog.append(title, description, label, actions); document.body.append(dialog); dialog.showModal();
  }));
  dialogQueue = pending.then(() => undefined); return pending;
}
async function cancel(job) {
  job.state = 'cancelled'; job.controller?.abort(); job.message = 'Annulation…'; render();
  try { if (job.id) await request(`/uploads/${job.id}`, { method: 'DELETE' }); job.file = null; job.message = 'Annulé'; }
  catch (e) { job.state = 'error'; job.message = e.message; }
  render();
}
function render() {
  const panel = $('transfer-panel'), list = $('transfer-list'); if (!panel || !list) return;
  panel.hidden = !jobs.size; list.replaceChildren();
  for (const job of jobs.values()) {
    const row = document.createElement('div'); row.className = 'transfer-row';
    const title = document.createElement('strong'); title.textContent = job.path.split('/').pop();
    const info = document.createElement('small');
    const seconds = (performance.now() - job.started) / 1000;
    const speed = seconds > 0 && job.state === 'running' ? Math.max(0, job.offset - (job.initialOffset || 0)) / seconds : 0;
    const eta = speed ? Math.ceil((job.size - job.offset) / speed) : null;
    info.textContent = `${job.message} · ${human(job.offset)} / ${human(job.size)}${speed ? ` · ${human(speed)}/s · ${eta}s restantes` : ''}`;
    const progress = document.createElement('progress'); progress.max = job.size || 1; progress.value = job.size ? job.offset : job.state === 'complete' ? 1 : 0;
    progress.setAttribute('aria-label', `Octets confirmés pour ${title.textContent}`);
    const actions = document.createElement('div'); actions.className = 'transfer-actions';
    const button = (text, action) => { const b = document.createElement('button'); b.textContent = text; b.onclick = action; actions.append(b); };
    if (job.state === 'running') button('Pause', () => { job.state = 'paused'; job.message = 'En pause'; job.controller.abort(); render(); });
    if (['paused', 'error'].includes(job.state) && job.file) button('Reprendre', () => { job.state = 'queued'; pump(); render(); });
    if (!['complete', 'cancelled', 'skipped'].includes(job.state)) button('Annuler', () => cancel(job));
    else button('Fermer', () => { jobs.delete(job.localId); if (job.id) request(`/uploads/${job.id}`, { method: 'DELETE' }).catch(() => {}); render(); });
    row.append(title, info, progress, actions); list.append(row);
  }
}
async function* readEntry(entry, prefix = '') {
  if (entry.isFile) { yield { file: await new Promise((ok, fail) => entry.file(ok, fail)), relative: `${prefix}${entry.name}` }; }
  else if (entry.isDirectory) {
    const reader = entry.createReader();
    while (true) { const children = await new Promise((ok, fail) => reader.readEntries(ok, fail)); if (!children.length) break; for (const child of children) yield* readEntry(child, `${prefix}${entry.name}/`); }
  }
}
export function install() {
  if (installed) return; installed = true;
  window.addEventListener('beforeunload', (event) => { if (dirty || [...jobs.values()].some(j => ['running', 'queued', 'paused'].includes(j.state))) { event.preventDefault(); event.returnValue = ''; } });
  let depth = 0;
  const external = (e) => [...(e.dataTransfer?.types || [])].includes('Files');
  document.addEventListener('dragstart', (e) => {
    const card = e.target.closest?.('[data-entry]'); if (!card) return;
    if (!card.classList.contains('selected')) card.click();
    e.dataTransfer.setData('application/x-glassftp', 'selection'); e.dataTransfer.effectAllowed = 'move';
  });
  document.addEventListener('dragenter', (e) => { if (external(e)) { e.preventDefault(); depth++; $('dropzone')?.classList.add('dropping'); } });
  document.addEventListener('dragleave', (e) => { if (external(e) && --depth <= 0) { depth = 0; $('dropzone')?.classList.remove('dropping'); } });
  document.addEventListener('dragover', (e) => {
    if (!external(e) && ![...(e.dataTransfer?.types || [])].includes('application/x-glassftp')) return;
    e.preventDefault(); e.dataTransfer.dropEffect = external(e) ? 'copy' : 'move';
    document.querySelectorAll('.drop-target').forEach(el => el.classList.remove('drop-target'));
    const folder = e.target.closest?.('[data-folder]'); if (folder?.dataset.folder) folder.classList.add('drop-target');
  });
  document.addEventListener('drop', async (e) => {
    if (!external(e) && ![...(e.dataTransfer?.types || [])].includes('application/x-glassftp')) return;
    e.preventDefault(); depth = 0; $('dropzone')?.classList.remove('dropping');
    document.querySelectorAll('.drop-target').forEach(el => el.classList.remove('drop-target'));
    const target = e.target.closest?.('[data-folder]')?.dataset.folder || $('glassftp').dataset.path;
    if (!external(e)) { window.dispatchEvent(new CustomEvent('glassftp-move', { detail: target })); return; }
    collisionChoice = null;
    // Capture handles synchronously; DataTransfer is cleared after this event returns.
    const entries = [...(e.dataTransfer.items || [])].map(i => i.webkitGetAsEntry?.()).filter(Boolean);
    const files = [...e.dataTransfer.files];
    try {
      if (entries.length) for (const entry of entries) for await (const { file, relative } of readEntry(entry)) { await ensureParents(target, relative); enqueue(file, join(target, relative)); }
      else for (const file of files) enqueue(file, join(target, file.name));
    } catch (e) { showFailure('Import du dossier', e.message); }
  });
  $('minimize-transfers').onclick = () => $('transfer-panel').classList.toggle('minimized');
  const events = new EventSource('/api/v1/events');
  events.onopen = () => { $('connection-label').textContent = 'Connecté'; document.documentElement.dataset.connection = 'online'; notify('*'); };
  events.onerror = () => { $('connection-label').textContent = 'Reconnexion…'; document.documentElement.dataset.connection = 'offline'; };
  events.addEventListener('change', (e) => { try { notify(JSON.parse(e.data).path); } catch {} });
  window.addEventListener('focus', () => notify('*'));
  setInterval(() => { if (!document.hidden && !dirty) notify('*'); }, 60000);
  request('/uploads').then(items => { for (const u of items) { const localId = u.id; jobs.set(localId, { ...u, localId, state: 'error', message: u.status === 'committing' ? 'Finalisation à vérifier côté serveur' : 'Session précédente — reprise disponible via l’API, ou annulez puis réimportez' }); } render(); }).catch(e => showFailure('Transferts en attente', e.message));
}
