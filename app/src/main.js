const { core, dialog, fs } = window.__TAURI__;

// Keep in sync with MAX_LINKS in src-tauri/src/lib.rs.
const MAX_LINKS = 16;
// Keep in sync with MAX_CARDS in src-tauri/src/lib.rs.
const MAX_CARDS = 4;
// Cap on the canonical profile's field list — keep in sync with
// MAX_PROFILE_FIELDS in src-tauri/src/lib.rs.
const MAX_PROFILE_FIELDS = 20;

const cardsView = document.getElementById('cards-view');
const editView = document.getElementById('edit-view');
const cardView = document.getElementById('card-view');
const cardsGrid = document.getElementById('cards-grid');
const form = document.getElementById('card-form');
const statusMsg = document.getElementById('status-msg');
const photoPreview = document.getElementById('photo-preview');
const choosePhotoBtn = document.getElementById('choose-photo-btn');
const cancelEditBtn = document.getElementById('cancel-edit-btn');
const deleteCardBtn = document.getElementById('delete-card-btn');
const linkEntriesEl = document.getElementById('link-entries');
const newLinkLabel = document.getElementById('new-link-label');
const newLinkUrl = document.getElementById('new-link-url');
const addLinkBtn = document.getElementById('add-link-btn');
const linkLimitHint = document.getElementById('link-limit-hint');
linkLimitHint.textContent = `Up to ${MAX_LINKS} links — remove one to add another.`;
const importBizbuzBtn = document.getElementById('import-bizbuz-btn');
const socialsGrid = document.getElementById('socials-grid');
const cardSocialsEl = document.getElementById('card-socials');
const urlImportModal = document.getElementById('url-import-modal');
const urlImportInput = document.getElementById('url-import-input');
const urlImportSubmitBtn = document.getElementById('url-import-submit-btn');
const urlImportCancelBtn = document.getElementById('url-import-cancel-btn');
const publishBtn = document.getElementById('publish-btn');
const linkRow = document.getElementById('link-row');
const linkText = document.getElementById('link-text');
const copyLinkBtn = document.getElementById('copy-link-btn');
const shareBtn = document.getElementById('share-btn');
const shareAppGroupBtn = document.getElementById('share-app-group-btn');
const shareAppGroupHint = document.getElementById('share-app-group-hint');
const referralShareBtn = document.getElementById('referral-share-btn');
const resetAllBtn = document.getElementById('reset-all-btn');
const profileNavBtn = document.getElementById('profile-nav-btn');
const backToCardsBtn = document.getElementById('back-to-cards-btn');
const profileView = document.getElementById('profile-view');
const profileForm = document.getElementById('canonical-profile-form');
const profilePhotoPreview = document.getElementById('profile-photo-preview');
const profileChoosePhotoBtn = document.getElementById('profile-choose-photo-btn');
const profilePullBtn = document.getElementById('profile-pull-btn');
const profileFieldsEl = document.getElementById('profile-fields');
const profileNewFieldName = document.getElementById('profile-new-field-name');
const profileNewFieldValue = document.getElementById('profile-new-field-value');
const profileAddFieldBtn = document.getElementById('profile-add-field-btn');
const profileFieldLimitHint = document.getElementById('profile-field-limit-hint');
const profileCloseBtn = document.getElementById('profile-close-btn');
const chooserModal = document.getElementById('new-card-chooser');
const chooserImportBtn = document.getElementById('chooser-import-btn');
const chooserBuildBtn = document.getElementById('chooser-build-btn');
const chooserCancelBtn = document.getElementById('chooser-cancel-btn');

const PHOTO_SIZE = 480;
const PHOTO_QUALITY = 0.85;

let cards = [];
let activeCard = null; // the card shown in card-view / being edited in edit-view (null = creating new)
let pendingPhoto = null; // base64 JPEG (no data: prefix), staged from the picker until Save
let pendingLinks = []; // working NON-social link-entry array while the edit form is open — socials live in pendingSocials, merged in cardFromForm
let pendingSocials = {}; // slug -> username, staged from the socials grid; extracted from card.links on load, merged back on save
let expandedSocial = null; // slug of the currently-open disclosure, or null
let deleteArmed = false;
let deleteArmedTimeout = null;
// Id of the card currently in the App Group (the one BizBuz imports), or
// null if nothing is shared. Mirrors get_shared_card_id in Rust — kept in
// memory so the cards grid and card view can both show it without a round
// trip on every render.
let sharedCardId = null;

// Socials icon grid — same 8 platforms as BizBuz. SVG paths from Simple
// Icons (via the PLATFORMS array below). Codeberg uses a letter monogram
// since it's not in Simple Icons. Users tap an icon, enter their handle,
// and the entry is upserted into the card's link list (with the platform's
// standard URL template).
const SOCIALS = [
    { slug: 'instagram', label: 'Instagram', hex: '#E4405F', viewBox: 24,
      path: 'M7.0301.084c-1.2768.0602-2.1487.264-2.911.5634-.7888.3075-1.4575.72-2.1228 1.3877-.6652.6677-1.075 1.3368-1.3802 2.127-.2954.7638-.4956 1.6365-.552 2.914-.0564 1.2775-.0689 1.6882-.0626 4.947.0062 3.2586.0206 3.6671.0825 4.9473.061 1.2765.264 2.1482.5635 2.9107.308.7889.72 1.4573 1.388 2.1228.6679.6655 1.3365 1.0743 2.1285 1.38.7632.295 1.6361.4961 2.9134.552 1.2773.056 1.6884.069 4.9462.0627 3.2578-.0062 3.668-.0207 4.9478-.0814 1.28-.0607 2.147-.2652 2.9098-.5633.7889-.3086 1.4578-.72 2.1228-1.3881.665-.6682 1.0745-1.3378 1.3795-2.1284.2957-.7632.4966-1.636.552-2.9124.056-1.2809.0692-1.6898.063-4.948-.0063-3.2583-.021-3.6668-.0817-4.9465-.0607-1.2797-.264-2.1487-.5633-2.9117-.3084-.7889-.72-1.4568-1.3876-2.1228C21.2982 1.33 20.628.9208 19.8378.6165 19.074.321 18.2017.1197 16.9244.0645 15.6471.0093 15.236-.005 11.977.0014 8.718.0076 8.31.0215 7.0301.0839m.1402 21.6932c-1.17-.0509-1.8053-.2453-2.2287-.408-.5606-.216-.96-.4771-1.3819-.895-.422-.4178-.6811-.8186-.9-1.378-.1644-.4234-.3624-1.058-.4171-2.228-.0595-1.2645-.072-1.6442-.079-4.848-.007-3.2037.0053-3.583.0607-4.848.05-1.169.2456-1.805.408-2.2282.216-.5613.4762-.96.895-1.3816.4188-.4217.8184-.6814 1.3783-.9003.423-.1651 1.0575-.3614 2.227-.4171 1.2655-.06 1.6447-.072 4.848-.079 3.2033-.007 3.5835.005 4.8495.0608 1.169.0508 1.8053.2445 2.228.408.5608.216.96.4754 1.3816.895.4217.4194.6816.8176.9005 1.3787.1653.4217.3617 1.056.4169 2.2263.0602 1.2655.0739 1.645.0796 4.848.0058 3.203-.0055 3.5834-.061 4.848-.051 1.17-.245 1.8055-.408 2.2294-.216.5604-.4763.96-.8954 1.3814-.419.4215-.8181.6811-1.3783.9-.4224.1649-1.0577.3617-2.2262.4174-1.2656.0595-1.6448.072-4.8493.079-3.2045.007-3.5825-.006-4.848-.0608M16.953 5.5864A1.44 1.44 0 1 0 18.39 4.144a1.44 1.44 0 0 0-1.437 1.4424M5.8385 12.012c.0067 3.4032 2.7706 6.1557 6.173 6.1493 3.4026-.0065 6.157-2.7701 6.1506-6.1733-.0065-3.4032-2.771-6.1565-6.174-6.1498-3.403.0067-6.156 2.771-6.1496 6.1738M8 12.0077a4 4 0 1 1 4.008 3.9921A3.9996 3.9996 0 0 1 8 12.0077' },
    { slug: 'x', label: 'X', hex: '#000000', viewBox: 24,
      path: 'M14.234 10.162 22.977 0h-2.072l-7.591 8.824L7.251 0H.258l9.168 13.343L.258 24H2.33l8.016-9.318L16.749 24h6.993zm-2.837 3.299-.929-1.329L3.076 1.56h3.182l5.965 8.532.929 1.329 7.754 11.09h-3.182z' },
    { slug: 'tiktok', label: 'TikTok', hex: '#000000', viewBox: 24,
      path: 'M12.525.02c1.31-.02 2.61-.01 3.91-.02.08 1.53.63 3.09 1.75 4.17 1.12 1.11 2.7 1.62 4.24 1.79v4.03c-1.44-.05-2.89-.35-4.2-.97-.57-.26-1.1-.59-1.62-.93-.01 2.92.01 5.84-.02 8.75-.08 1.4-.54 2.79-1.35 3.94-1.31 1.92-3.58 3.17-5.91 3.21-1.43.08-2.86-.31-4.08-1.03-2.02-1.19-3.44-3.37-3.65-5.71-.02-.5-.03-1-.01-1.49.18-1.9 1.12-3.72 2.58-4.96 1.66-1.44 3.98-2.13 6.15-1.72.02 1.48-.04 2.96-.04 4.44-.99-.32-2.15-.23-3.02.37-.63.41-1.11 1.04-1.36 1.75-.21.51-.15 1.07-.14 1.61.24 1.64 1.82 3.02 3.5 2.87 1.12-.01 2.19-.66 2.77-1.61.19-.33.4-.67.41-1.06.1-1.79.06-3.57.07-5.36.01-4.03-.01-8.05.02-12.07z' },
    { slug: 'youtube', label: 'YouTube', hex: '#FF0000', viewBox: 24,
      path: 'M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z' },
    { slug: 'facebook', label: 'Facebook', hex: '#0866FF', viewBox: 24,
      path: 'M9.101 23.691v-7.98H6.627v-3.667h2.474v-1.58c0-4.085 1.848-5.978 5.858-5.978.401 0 .955.042 1.468.103a8.68 8.68 0 0 1 1.141.195v3.325a8.623 8.623 0 0 0-.653-.036 26.805 26.805 0 0 0-.733-.009c-.707 0-1.259.096-1.675.309a1.686 1.686 0 0 0-.679.622c-.258.42-.374.995-.374 1.752v1.297h3.919l-.386 2.103-.287 1.564h-3.246v8.245C19.396 23.238 24 18.179 24 12.044c0-6.627-5.373-12-12-12s-12 5.373-12 12c0 5.628 3.874 10.35 9.101 11.647Z' },
    { slug: 'linkedin', label: 'LinkedIn', hex: '#0A66C2', viewBox: 448,
      path: 'M416 32L31.9 32C14.3 32 0 46.5 0 64.3L0 447.7C0 465.5 14.3 480 31.9 480L416 480c17.6 0 32-14.5 32-32.3l0-383.4C448 46.5 433.6 32 416 32zM135.4 416l-66.4 0 0-213.8 66.5 0 0 213.8-.1 0zM102.2 96a38.5 38.5 0 1 1 0 77 38.5 38.5 0 1 1 0-77zM384.3 416l-66.4 0 0-104c0-24.8-.5-56.7-34.5-56.7-34.6 0-39.9 27-39.9 54.9l0 105.8-66.4 0 0-213.8 63.7 0 0 29.2 .9 0c8.9-16.8 30.6-34.5 62.9-34.5 67.2 0 79.7 44.3 79.7 101.9l0 117.2z' },
    { slug: 'github', label: 'GitHub', hex: '#181717', viewBox: 24,
      path: 'M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12' },
    { slug: 'codeberg', label: 'Codeberg', hex: '#2185D0', viewBox: 24, letter: 'C' },
];

function socialUrlFor(slug, handle) {
    switch (slug) {
        case 'instagram': return `https://instagram.com/${encodeURIComponent(handle)}`;
        case 'x':         return `https://x.com/${encodeURIComponent(handle)}`;
        case 'tiktok':    return `https://tiktok.com/@${encodeURIComponent(handle)}`;
        case 'youtube':   return `https://youtube.com/@${encodeURIComponent(handle)}`;
        case 'facebook':  return `https://facebook.com/${encodeURIComponent(handle)}`;
        case 'linkedin':  return `https://linkedin.com/in/${encodeURIComponent(handle)}`;
        case 'github':    return `https://github.com/${encodeURIComponent(handle)}`;
        case 'codeberg':  return `https://codeberg.org/${encodeURIComponent(handle)}`;
        default: return '#';
    }
}

function socialIconHtml(social) {
    if (social.path) {
        return `<svg viewBox="0 0 ${social.viewBox} ${social.viewBox}" xmlns="http://www.w3.org/2000/svg" aria-hidden="true"><path d="${social.path}"/></svg>`;
    }
    return `<span class="letter-mono">${social.letter}</span>`;
}

// JS mirror of Rust's `social_field_for_url` in bizbuz — recognises the same
// 8 platforms so we can round-trip socials in and out of the card's `links`
// array without losing structure.
function detectSocialSlug(url) {
    if (!url) return null;
    let host = '';
    try {
        const u = new URL(url.startsWith('http') ? url : `https://${url}`);
        host = u.hostname.replace(/^www\./, '').toLowerCase();
    } catch { return null; }
    switch (host) {
        case 'instagram.com': return 'instagram';
        case 'x.com': case 'twitter.com': return 'x';
        case 'tiktok.com': return 'tiktok';
        case 'youtube.com': case 'youtu.be': return 'youtube';
        case 'facebook.com': case 'fb.com': return 'facebook';
        case 'linkedin.com': return 'linkedin';
        case 'github.com': return 'github';
        case 'codeberg.org': return 'codeberg';
        default: return null;
    }
}

function extractHandleFrom(url, slug) {
    try {
        const u = new URL(url.startsWith('http') ? url : `https://${url}`);
        let path = u.pathname.replace(/^\/+/, '').replace(/\/+$/, '');
        if (slug === 'linkedin') path = path.replace(/^in\//, '');
        return decodeURIComponent(path.split('/')[0] || '').replace(/^@/, '');
    } catch { return ''; }
}

// ── View / status helpers ────────────────────────────────────────────────────

function showView(name) {
    cardsView.hidden = name !== 'cards';
    editView.hidden = name !== 'edit';
    cardView.hidden = name !== 'card';
    profileView.hidden = name !== 'profile';
    profileNavBtn.hidden = name === 'profile';
    backToCardsBtn.hidden = name !== 'card';
}

let statusTimeout = null;
function setStatus(message) {
    statusMsg.textContent = message;
    statusMsg.classList.add('visible');
    clearTimeout(statusTimeout);
    statusTimeout = setTimeout(() => statusMsg.classList.remove('visible'), 2500);
}

function getInitials(name) {
    if (!name) return '?';
    return name
        .split(/\s+/)
        .filter(Boolean)
        .map((word) => word[0])
        .join('')
        .slice(0, 2)
        .toUpperCase();
}

function setAvatarContent(el, photo, name) {
    if (photo) {
        el.style.backgroundImage = `url(data:image/jpeg;base64,${photo})`;
        el.textContent = '';
    } else {
        el.style.backgroundImage = '';
        el.textContent = getInitials(name);
    }
}

// ── Photo picker ──────────────────────────────────────────────────────────────

async function resizeImageToJpegBase64(bytes) {
    const blob = new Blob([bytes]);
    const bitmap = await createImageBitmap(blob);

    const scale = Math.min(1, PHOTO_SIZE / Math.max(bitmap.width, bitmap.height));
    const width = Math.round(bitmap.width * scale);
    const height = Math.round(bitmap.height * scale);

    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext('2d');
    ctx.drawImage(bitmap, 0, 0, width, height);

    const dataUrl = canvas.toDataURL('image/jpeg', PHOTO_QUALITY);
    return dataUrl.split(',')[1];
}

choosePhotoBtn.addEventListener('click', async () => {
    try {
        const path = await dialog.open({
            multiple: false,
            filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'heic'] }],
        });
        if (!path) return;

        const bytes = await fs.readFile(path);
        pendingPhoto = await resizeImageToJpegBase64(bytes);
        setAvatarContent(photoPreview, pendingPhoto, '');
    } catch (err) {
        setStatus(`Couldn't set photo: ${err}`);
    }
});

// ── Link list editor ─────────────────────────────────────────────────────────

// Index of the link entry currently open for inline editing (tapped), or
// null if none — only one row edits at a time.
let editingLinkIndex = null;

function renderLinkEntries() {
    linkEntriesEl.innerHTML = '';

    pendingLinks.forEach((entry, index) => {
        const li = document.createElement('li');
        li.className = 'link-entry';

        if (index === editingLinkIndex) {
            const fields = document.createElement('div');
            fields.className = 'link-entry-edit-fields';

            // iOS capitalises the first letter of a text field by default,
            // which is wrong for both of these: a URL is case-sensitive after
            // the host, and a label is usually a lowercase brand ("github",
            // "etsy shop"). Matches the socials handle input below.
            const labelInput = document.createElement('input');
            labelInput.type = 'text';
            labelInput.autocapitalize = 'none';
            labelInput.autocomplete = 'off';
            labelInput.spellcheck = false;
            labelInput.placeholder = 'Label';
            labelInput.maxLength = 40;
            labelInput.value = entry.label;

            const urlInput = document.createElement('input');
            urlInput.type = 'text';
            urlInput.autocapitalize = 'none';
            urlInput.autocomplete = 'off';
            urlInput.spellcheck = false;
            urlInput.inputMode = 'url';
            urlInput.placeholder = 'URL';
            urlInput.value = entry.url;

            // Write through on every keystroke, so pendingLinks always matches
            // what's on screen. Edits used to land only on Enter or the ✓
            // button — tapping a row, fixing the URL and then tapping Save
            // (the obvious thing to do) never ran that commit, so Save
            // republished the old URL and the edit silently vanished. The
            // socials disclosure above already worked this way.
            const writeThrough = () => {
                entry.label = labelInput.value.trim();
                entry.url = urlInput.value.trim();
            };
            labelInput.addEventListener('input', writeThrough);
            urlInput.addEventListener('input', writeThrough);

            // Enter and ✓ now only close the editor; the data is already in.
            const commit = () => {
                writeThrough();
                editingLinkIndex = null;
                renderLinkEntries();
            };
            // preventDefault: Enter in a form input is also an implicit Save.
            const onEnter = (e) => { if (e.key === 'Enter') { e.preventDefault(); commit(); } };
            labelInput.addEventListener('keydown', onEnter);
            urlInput.addEventListener('keydown', onEnter);

            fields.append(labelInput, urlInput);
            li.appendChild(fields);

            const doneBtn = document.createElement('button');
            doneBtn.type = 'button';
            doneBtn.textContent = '✓';
            doneBtn.addEventListener('click', commit);

            const actions = document.createElement('div');
            actions.className = 'link-entry-actions';
            actions.appendChild(doneBtn);
            li.appendChild(actions);

            linkEntriesEl.appendChild(li);
            labelInput.focus();
            return;
        }

        const text = document.createElement('div');
        text.className = 'link-entry-text';
        text.innerHTML = '<div class="link-entry-label"></div><div class="link-entry-url"></div>';
        text.querySelector('.link-entry-label').textContent = entry.label || entry.url;
        text.querySelector('.link-entry-url').textContent = entry.url;
        text.addEventListener('click', () => {
            editingLinkIndex = index;
            renderLinkEntries();
        });
        li.appendChild(text);

        const actions = document.createElement('div');
        actions.className = 'link-entry-actions';

        const upBtn = document.createElement('button');
        upBtn.type = 'button';
        upBtn.textContent = '↑';
        upBtn.disabled = index === 0;
        upBtn.addEventListener('click', () => {
            [pendingLinks[index - 1], pendingLinks[index]] = [pendingLinks[index], pendingLinks[index - 1]];
            renderLinkEntries();
        });

        const downBtn = document.createElement('button');
        downBtn.type = 'button';
        downBtn.textContent = '↓';
        downBtn.disabled = index === pendingLinks.length - 1;
        downBtn.addEventListener('click', () => {
            [pendingLinks[index], pendingLinks[index + 1]] = [pendingLinks[index + 1], pendingLinks[index]];
            renderLinkEntries();
        });

        const removeBtn = document.createElement('button');
        removeBtn.type = 'button';
        removeBtn.textContent = '×';
        removeBtn.addEventListener('click', () => {
            pendingLinks.splice(index, 1);
            if (editingLinkIndex === index) editingLinkIndex = null;
            renderLinkEntries();
        });

        actions.append(upBtn, downBtn, removeBtn);
        li.appendChild(actions);
        linkEntriesEl.appendChild(li);
    });

    const atLimit = pendingLinks.length >= MAX_LINKS;
    addLinkBtn.disabled = atLimit;
    importBizbuzBtn.disabled = atLimit;
    linkLimitHint.hidden = !atLimit;
}

// Moves whatever is typed in the "Add Link" row into pendingLinks. Shared by
// the Add button and Save — a link typed but not added used to be thrown away
// on Save, since Save only sends the list. Returns false (after telling the
// user why) when the row holds something that can't be added.
function takeNewLink() {
    const label = newLinkLabel.value.trim();
    const url = newLinkUrl.value.trim();
    if (!label && !url) return true;
    if (!url) {
        setStatus(`Add a URL for "${label}".`);
        newLinkUrl.focus();
        return false;
    }
    if (pendingLinks.length >= MAX_LINKS) {
        setStatus(`Up to ${MAX_LINKS} links — remove one to add another.`);
        return false;
    }

    pendingLinks.push({ id: '', label, url });
    newLinkLabel.value = '';
    newLinkUrl.value = '';
    renderLinkEntries();
    return true;
}

addLinkBtn.addEventListener('click', () => {
    if (!newLinkLabel.value.trim() && !newLinkUrl.value.trim()) {
        setStatus('Type a URL first.');
        return;
    }
    takeNewLink();
});

// Enter in the Add row adds the link rather than submitting (saving) the form.
for (const input of [newLinkLabel, newLinkUrl]) {
    input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); addLinkBtn.click(); }
    });
}

// URL-import modal: step 2 of the Import flow (chooser → URL prompt → filled
// edit view). Splits imported links into socials (managed by the icon grid)
// and non-socials (the flat entries list), then opens the edit view.
async function performUrlImport(url) {
    try {
        const imported = await core.invoke('import_links', { url });
        let socialsAdded = 0;
        const nonSocialFresh = [];
        for (const l of imported) {
            const slug = detectSocialSlug(l.url);
            if (slug) {
                if (!pendingSocials[slug]) {
                    pendingSocials[slug] = extractHandleFrom(l.url, slug);
                    socialsAdded++;
                }
            } else {
                nonSocialFresh.push(l);
            }
        }
        const existingUrls = new Set(pendingLinks.map((l) => normalizeUrl(l.url)));
        const dedupedNonSocial = nonSocialFresh.filter((l) => {
            const key = normalizeUrl(l.url);
            if (existingUrls.has(key)) return false;
            existingUrls.add(key);
            return true;
        });
        const room = MAX_LINKS - pendingLinks.length;
        const toAdd = dedupedNonSocial.slice(0, room);
        pendingLinks.push(...toAdd.map((l) => ({ id: '', label: l.label, url: l.url })));
        renderSocialsGrid();
        renderLinkEntries();

        const totalAdded = socialsAdded + toAdd.length;
        if (totalAdded === 0) {
            setStatus('Nothing new to import from that URL.');
        } else if (toAdd.length < dedupedNonSocial.length) {
            setStatus(`Imported ${totalAdded} — some links were skipped to stay under the ${MAX_LINKS}-link limit.`);
        } else {
            setStatus(`Imported ${totalAdded} item${totalAdded === 1 ? '' : 's'}!`);
        }
        return true;
    } catch (err) {
        setStatus(`Couldn't import: ${err}`);
        return false;
    }
}

urlImportSubmitBtn.addEventListener('click', async () => {
    const url = urlImportInput.value.trim();
    if (!url) return;
    urlImportSubmitBtn.disabled = true;
    setStatus('Importing links…');
    try {
        const ok = await performUrlImport(url);
        if (ok) {
            urlImportModal.hidden = true;
            urlImportInput.value = '';
            showView('edit');
        }
    } finally {
        urlImportSubmitBtn.disabled = false;
    }
});

urlImportInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { e.preventDefault(); urlImportSubmitBtn.click(); }
});

urlImportCancelBtn.addEventListener('click', () => {
    urlImportModal.hidden = true;
    urlImportInput.value = '';
    // Back to the chooser so the user can pick Build instead.
    chooserModal.hidden = false;
});

// ── App Group sharing ────────────────────────────────────────────────────────
//
// Import fills name/bio/photo only if currently empty (additive, not
// destructive) and merges links through the same dedupe/cap pattern as the
// URL-import feature above.

importBizbuzBtn.addEventListener('click', async () => {
    if (pendingLinks.length >= MAX_LINKS) return;
    importBizbuzBtn.disabled = true;
    setStatus('Checking for a shared BizBuz profile…');
    try {
        const result = await core.invoke('import_from_bizbuz');
        const nameEl = document.getElementById('field-name');
        const bioEl = document.getElementById('field-bio');
        if (result.name && !nameEl.value.trim()) nameEl.value = result.name;
        if (result.bio && !bioEl.value.trim()) bioEl.value = result.bio;
        if (result.photo && !pendingPhoto) {
            pendingPhoto = result.photo;
            setAvatarContent(photoPreview, pendingPhoto, nameEl.value || '');
        }

        // Split imported links into socials (upsert into pendingSocials, no
        // dedupe against pendingLinks needed since socials live there) and
        // non-socials (dedupe as before).
        let socialsAdded = 0;
        const nonSocialFresh = [];
        for (const l of result.links) {
            const slug = detectSocialSlug(l.url);
            if (slug) {
                if (!pendingSocials[slug]) {
                    pendingSocials[slug] = extractHandleFrom(l.url, slug);
                    socialsAdded++;
                }
            } else {
                nonSocialFresh.push(l);
            }
        }
        const existingUrls = new Set(pendingLinks.map((l) => normalizeUrl(l.url)));
        const fresh = nonSocialFresh.filter((l) => {
            const key = normalizeUrl(l.url);
            if (existingUrls.has(key)) return false;
            existingUrls.add(key);
            return true;
        });
        const room = MAX_LINKS - pendingLinks.length;
        const toAdd = fresh.slice(0, room);
        pendingLinks.push(...toAdd.map((l) => ({ id: '', label: l.label, url: l.url })));
        renderSocialsGrid();
        renderLinkEntries();

        const notes = [];
        const total = toAdd.length + socialsAdded;
        if (total) notes.push(`Imported ${total} item${total === 1 ? '' : 's'} from BizBuz`);
        if (result.skippedFields.length) notes.push(`${result.skippedFields.length} field${result.skippedFields.length === 1 ? '' : 's'} couldn't be imported (${result.skippedFields.join(', ')} — no equivalent here)`);
        setStatus(notes.length ? notes.join(' — ') : 'Nothing new to import from BizBuz.');
    } catch (err) {
        setStatus(`${err}`); // Rust already returns a complete, user-facing sentence
    } finally {
        importBizbuzBtn.disabled = pendingLinks.length >= MAX_LINKS;
    }
});

// ── Form <-> LinkCard ─────────────────────────────────────────────────────────

function cardFromForm() {
    // Merge socials from the icon grid into the links array so the whole
    // set round-trips through the same Rust `LinkCard` shape. Socials come
    // first for stable ordering — they render as branded icons on the
    // published SVG anyway.
    const socialLinks = [];
    for (const s of SOCIALS) {
        const handle = pendingSocials[s.slug];
        if (handle && handle.trim()) {
            socialLinks.push({ id: '', label: s.label, url: socialUrlFor(s.slug, handle.trim()) });
        }
    }
    return {
        id: activeCard?.id || '',
        name: document.getElementById('field-name').value.trim() || undefined,
        bio: document.getElementById('field-bio').value.trim() || undefined,
        photo: pendingPhoto || undefined,
        links: [...socialLinks, ...pendingLinks],
    };
}

function fillForm(source) {
    document.getElementById('field-name').value = source?.name || '';
    document.getElementById('field-bio').value = source?.bio || '';
    pendingPhoto = source?.photo || null;

    // Separate stored links into socials (managed by the icon grid) and
    // free-form links (managed by the entries list). The first matching
    // handle per slug wins; subsequent duplicates stay in the flat list.
    pendingSocials = {};
    pendingLinks = [];
    for (const l of source?.links || []) {
        const slug = detectSocialSlug(l.url);
        if (slug && !pendingSocials[slug]) {
            pendingSocials[slug] = extractHandleFrom(l.url, slug);
        } else {
            pendingLinks.push({ ...l });
        }
    }

    editingLinkIndex = null;
    expandedSocial = null;
    // Save now adds whatever is left in the Add Link row, so text abandoned
    // while editing one card must not ride along into the next.
    newLinkLabel.value = '';
    newLinkUrl.value = '';
    setAvatarContent(photoPreview, source?.photo, source?.name || '');
    renderSocialsGrid();
    renderLinkEntries();
}

// ── Socials grid + disclosure (Edit view) ────────────────────────────────

function renderSocialsGrid() {
    // Clear any prior disclosure — it lives as a sibling inserted after the
    // grid, so a fresh render needs to remove the stale one first.
    const staleDisclosure = document.getElementById('social-disclosure');
    if (staleDisclosure) staleDisclosure.remove();

    socialsGrid.innerHTML = '';
    for (const social of SOCIALS) {
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'social-icon-btn';
        if (pendingSocials[social.slug]) btn.classList.add('filled');
        if (expandedSocial === social.slug) btn.classList.add('expanded');
        btn.title = social.label;
        btn.setAttribute('aria-label', social.label);
        btn.innerHTML = socialIconHtml(social);
        btn.addEventListener('click', () => {
            expandedSocial = expandedSocial === social.slug ? null : social.slug;
            renderSocialsGrid();
        });
        socialsGrid.appendChild(btn);
    }
    if (expandedSocial) renderSocialDisclosure();
}

function renderSocialDisclosure() {
    const social = SOCIALS.find((s) => s.slug === expandedSocial);
    if (!social) return;

    const box = document.createElement('div');
    box.className = 'social-disclosure';
    box.id = 'social-disclosure';

    const title = document.createElement('span');
    title.className = 'social-disclosure-title';
    title.textContent = `${social.label} username`;
    box.appendChild(title);

    const body = document.createElement('div');
    body.className = 'social-disclosure-body';

    const input = document.createElement('input');
    input.type = 'text';
    input.placeholder = 'username';
    input.autocapitalize = 'none';
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.value = pendingSocials[social.slug] || '';
    const commit = () => {
        const cleaned = input.value.trim().replace(/^@/, '');
        if (cleaned) pendingSocials[social.slug] = cleaned;
        else delete pendingSocials[social.slug];
        // Live-toggle the icon's filled state without a full re-render (would
        // steal focus from the input).
        const btn = socialsGrid.children[SOCIALS.indexOf(social)];
        btn.classList.toggle('filled', !!pendingSocials[social.slug]);
    };
    input.addEventListener('input', commit);
    input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); doneBtn.click(); }
    });

    const doneBtn = document.createElement('button');
    doneBtn.type = 'button';
    doneBtn.className = 'btn btn-secondary';
    doneBtn.textContent = 'Done';
    doneBtn.addEventListener('click', () => {
        commit();
        expandedSocial = null;
        renderSocialsGrid();
    });

    body.append(input, doneBtn);
    box.appendChild(body);

    socialsGrid.after(box);
    input.focus();
}

// ── Cards grid ────────────────────────────────────────────────────────────────

function renderCardsGrid() {
    cardsGrid.innerHTML = '';

    for (const c of cards) {
        const tile = document.createElement('button');
        tile.type = 'button';
        tile.className = 'card-tile filled';

        const avatar = document.createElement('div');
        avatar.className = 'avatar';
        setAvatarContent(avatar, c.photo, c.name || '');
        tile.appendChild(avatar);

        const name = document.createElement('span');
        name.className = 'card-tile-name';
        name.textContent = c.name || 'Untitled';
        tile.appendChild(name);

        // Only one card can be in the App Group at a time, so at most one
        // tile ever carries this badge.
        if (c.id === sharedCardId) {
            const badge = document.createElement('span');
            badge.className = 'card-tile-badge';
            badge.textContent = 'Shared';
            badge.title = 'Other apps import this card';
            tile.appendChild(badge);
        }

        tile.addEventListener('click', () => openCard(c));
        cardsGrid.appendChild(tile);
    }

    if (cards.length < MAX_CARDS) {
        const addTile = document.createElement('button');
        addTile.type = 'button';
        addTile.className = 'card-tile empty';
        addTile.textContent = '+';
        addTile.addEventListener('click', openNewCardForm);
        cardsGrid.appendChild(addTile);
    }
}

// ── Card view ─────────────────────────────────────────────────────────────────

// Mirrors normalize_url in src-tauri/src/lib.rs — recognizes a malformed
// scheme (missing colon, single slash) and replaces it with a clean one,
// rather than blindly prepending "https://" in front of it.
function normalizeUrl(url) {
    const trimmed = url.trim();
    const lower = trimmed.toLowerCase();

    let len = null;
    let scheme = null;
    if (lower.startsWith('https')) {
        len = 5;
        scheme = 'https://';
    } else if (lower.startsWith('http')) {
        len = 4;
        scheme = 'http://';
    }

    if (len !== null) {
        const rest = trimmed.slice(len);
        if (rest.startsWith(':') || rest.startsWith('/')) {
            return scheme + rest.replace(/^[:/]+/, '');
        }
    }

    return `https://${trimmed}`;
}

function renderCardView(c) {
    setAvatarContent(document.getElementById('card-avatar'), c.photo, c.name || '');
    document.getElementById('card-name').textContent = c.name || 'Untitled';

    const bioEl = document.getElementById('card-bio');
    bioEl.textContent = c.bio ? `"${c.bio}"` : '';
    bioEl.hidden = !c.bio;

    // Split the stored links into socials (rendered as branded icons above
    // the flat list) and non-social entries (the traditional row list).
    cardSocialsEl.innerHTML = '';
    const linksEl = document.getElementById('card-links');
    linksEl.innerHTML = '';
    const seenSocials = new Set();
    for (const entry of c.links || []) {
        if (!entry.url) continue;
        const slug = detectSocialSlug(entry.url);
        if (slug && !seenSocials.has(slug)) {
            seenSocials.add(slug);
            const social = SOCIALS.find((s) => s.slug === slug);
            const a = document.createElement('a');
            a.className = 'card-social-link';
            a.href = normalizeUrl(entry.url);
            a.target = '_blank';
            a.rel = 'noopener noreferrer';
            a.style.color = social.hex;
            const handle = extractHandleFrom(entry.url, slug);
            a.title = handle ? `${social.label} — @${handle}` : social.label;
            a.setAttribute('aria-label', a.title);
            a.innerHTML = socialIconHtml(social);
            cardSocialsEl.appendChild(a);
            continue;
        }
        const href = normalizeUrl(entry.url);
        const a = document.createElement('a');
        a.className = 'link-list-item';
        a.href = href;
        a.target = '_blank';
        a.rel = 'noreferrer';
        a.innerHTML = '<span></span><span class="arrow">→</span>';
        a.querySelector('span').textContent = entry.label || entry.url;
        linksEl.appendChild(a);
    }
}

function renderPublishLink(c) {
    if (c.shareUrl) {
        linkText.textContent = c.shareUrl;
        linkRow.hidden = false;
        publishBtn.textContent = 'Update Shareable Link';
    } else {
        linkRow.hidden = true;
        publishBtn.textContent = 'Get Shareable Link';
    }
}

// Upserts a card into the in-memory `cards` list and, if it's the one
// currently on screen, refreshes the link row too — used by both the
// explicit Publish button and the silent background sync below.
function applyCardUpdate(updated) {
    cards = cards.filter((c) => c.id !== updated.id);
    cards.push(updated);
    if (activeCard?.id === updated.id) {
        activeCard = updated;
        renderPublishLink(updated);
    }
}

// The shareable link (shareUrl, a pre-signed savage URL) is available the
// instant publish_card returns — the signature never expires, so it's
// computed once and doesn't need to be refreshed on later shares. Publishing
// proactively in the background (on boot and after every edit, not only when
// the user taps the button) keeps the published card in sync with the latest
// edits without requiring an explicit re-publish tap first.
async function backgroundPublishCard(cardId) {
    try {
        const updated = await core.invoke('publish_card', { cardId });
        applyCardUpdate(updated);
    } catch {
        // Best-effort — network hiccups etc. shouldn't surface to the user
        // for a sync they didn't explicitly ask for.
    }
}

function backgroundPublishAllCards() {
    for (const c of cards) backgroundPublishCard(c.id);
}

// ── App Group sharing ────────────────────────────────────────────────────────
//
// The App Group holds one Linkitylink card, which is what BizBuz's "Import
// from Linkitylink" reads. Which card that is, is the user's explicit
// choice — sharing only ever happens on a button tap, and the card list
// badges whichever card is currently shared so the choice stays visible.

async function refreshSharedCardId() {
    try {
        sharedCardId = await core.invoke('get_shared_card_id');
    } catch {
        // Treated as "nothing shared" — the badge and hint just stay off
        // rather than blocking the card list on an App Group read.
        sharedCardId = null;
    }
}

function renderShareState(c) {
    const isShared = !!c && c.id === sharedCardId;
    shareAppGroupBtn.textContent = isShared ? 'Update Shared Copy' : 'Share to Other Apps';
    if (isShared) {
        shareAppGroupHint.textContent = 'Other apps import this card. Tap to push your latest edits.';
    } else if (sharedCardId) {
        shareAppGroupHint.textContent = 'Another card is shared right now — this will replace it.';
    } else {
        shareAppGroupHint.textContent = 'Let BizBuz and your other apps import this card.';
    }
}

shareAppGroupBtn.addEventListener('click', async () => {
    if (!activeCard) return;
    shareAppGroupBtn.disabled = true;
    try {
        await core.invoke('share_card_to_app_group', { cardId: activeCard.id });
        sharedCardId = activeCard.id;
        renderShareState(activeCard);
        renderCardsGrid();
        setStatus('Shared — BizBuz can now import this card.');
    } catch (err) {
        setStatus(`Couldn't share: ${err}`);
    } finally {
        shareAppGroupBtn.disabled = false;
    }
});

function openCard(c) {
    activeCard = c;
    renderCardView(c);
    renderPublishLink(c);
    renderShareState(c);
    showView('card');
}

// Sets up the edit view for a fresh card and shows the chooser modal on top
// so the user picks Import-from-URL vs. Build-from-scratch first.
function openNewCardForm() {
    activeCard = null;
    pendingPhoto = null;
    pendingLinks = [];
    pendingSocials = {};
    expandedSocial = null;
    editingLinkIndex = null;
    disarmDelete();
    form.reset();
    setAvatarContent(photoPreview, null, '');
    renderSocialsGrid();
    renderLinkEntries();
    cancelEditBtn.hidden = cards.length === 0;
    deleteCardBtn.hidden = true;
    chooserModal.hidden = false;
}

chooserImportBtn.addEventListener('click', () => {
    chooserModal.hidden = true;
    urlImportModal.hidden = false;
    // Small delay so iOS actually focuses the input (right after a
    // hidden→visible transition, focus() sometimes gets swallowed).
    setTimeout(() => urlImportInput.focus(), 50);
});

chooserBuildBtn.addEventListener('click', () => {
    chooserModal.hidden = true;
    showView('edit');
});

chooserCancelBtn.addEventListener('click', () => {
    chooserModal.hidden = true;
    // If the user cancels the chooser and had no cards to begin with, drop
    // them on the (empty) cards grid so they can tap "+" to retry.
    if (cards.length === 0) showView('cards');
});

async function loadCards() {
    cards = await core.invoke('load_cards');
    renderCardsGrid();
    showView('cards');
    if (cards.length === 0) openNewCardForm();
}

form.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (!takeNewLink()) return;
    try {
        const saved = await core.invoke('save_card', { card: cardFromForm() });
        cards = cards.filter((c) => c.id !== saved.id);
        cards.push(saved);
        activeCard = saved;
        renderCardsGrid();
        renderCardView(saved);
        renderPublishLink(saved);
        renderShareState(saved);
        showView('card');
        backgroundPublishCard(saved.id);
    } catch (err) {
        setStatus(`Couldn't save: ${err}`);
    }
});

cancelEditBtn.addEventListener('click', () => {
    showView(activeCard ? 'card' : 'cards');
});

function disarmDelete() {
    deleteArmed = false;
    clearTimeout(deleteArmedTimeout);
    deleteCardBtn.textContent = 'Delete Card';
}

deleteCardBtn.addEventListener('click', async () => {
    if (!activeCard) return;

    if (!deleteArmed) {
        deleteArmed = true;
        deleteCardBtn.textContent = 'Tap again to confirm';
        deleteArmedTimeout = setTimeout(disarmDelete, 3000);
        return;
    }

    disarmDelete();
    try {
        setStatus('Deleting…');
        await core.invoke('delete_card', { id: activeCard.id });
        cards = cards.filter((c) => c.id !== activeCard.id);
        // delete_card stops sharing a card it just deleted, so drop the
        // badge here to match rather than leaving it on a gone card.
        if (sharedCardId === activeCard.id) sharedCardId = null;
        activeCard = null;
        renderCardsGrid();
        showView('cards');
        if (cards.length === 0) openNewCardForm();
    } catch (err) {
        // Rust returns a complete sentence here (it explains nothing was lost
        // locally and the delete can be retried), so pass it through.
        setStatus(`${err}`);
    }
});

document.getElementById('edit-btn').addEventListener('click', () => {
    if (activeCard) fillForm(activeCard);
    disarmDelete();
    cancelEditBtn.hidden = false;
    deleteCardBtn.hidden = false;
    showView('edit');
});

backToCardsBtn.addEventListener('click', () => {
    activeCard = null;
    renderCardsGrid();
    showView('cards');
});

publishBtn.addEventListener('click', async () => {
    if (!activeCard) return;
    publishBtn.disabled = true;
    setStatus('Publishing…');
    try {
        const updated = await core.invoke('publish_card', { cardId: activeCard.id });
        applyCardUpdate(updated);
        setStatus('Link ready!');
    } catch (err) {
        setStatus(`Couldn't publish: ${err}`);
    } finally {
        publishBtn.disabled = false;
    }
});

copyLinkBtn.addEventListener('click', async () => {
    try {
        await navigator.clipboard.writeText(linkText.textContent);
        setStatus('Copied!');
    } catch (err) {
        setStatus(`Couldn't copy: ${err}`);
    }
});

shareBtn.addEventListener('click', async () => {
    if (!activeCard) return;
    shareBtn.disabled = true;
    try {
        const url = activeCard.shareUrl || (await core.invoke('publish_card', { cardId: activeCard.id })).shareUrl;
        await core.invoke('plugin:share-sheet|share_text', { text: url });
    } catch (err) {
        setStatus(`Couldn't share: ${err}`);
    } finally {
        shareBtn.disabled = false;
    }
});

// ── Delete all data ──────────────────────────────────────────────────────────
//
// Two-tap confirm, same UX as delete-card. Unpublishes every card and the
// referral record before clearing anything locally, so it needs a connection —
// Rust refuses rather than orphaning published records it could no longer
// reach. Does not touch the App-Group canonical profile, which is shared with
// the sibling apps.
let resetArmed = false;
let resetArmedTimeout = null;

function disarmReset() {
    resetArmed = false;
    resetAllBtn.textContent = 'Delete All My Data';
    if (resetArmedTimeout) {
        clearTimeout(resetArmedTimeout);
        resetArmedTimeout = null;
    }
}

resetAllBtn.addEventListener('click', async () => {
    if (!resetArmed) {
        resetArmed = true;
        resetAllBtn.textContent = 'Tap again to confirm';
        resetArmedTimeout = setTimeout(disarmReset, 3000);
        return;
    }
    disarmReset();
    resetAllBtn.disabled = true;
    setStatus('Deleting your published cards…');
    try {
        await core.invoke('reset_all_data');
        cards = [];
        activeCard = null;
        sharedCardId = null;
        cards = await core.invoke('load_cards');
        renderCardsGrid();
        showView('cards');
        if (cards.length === 0) openNewCardForm();
        setStatus('Deleted — nothing of yours is published any more.');
    } catch (err) {
        setStatus(`${err}`);
    } finally {
        resetAllBtn.disabled = false;
    }
});

// ── Referral link ─────────────────────────────────────────────────────────────
//
// No preview/copy row, just a button. Tapping it asks Rust for this
// install's referral link (registering one with BDO/savage on first launch,
// reusing the cached one after that) and hands it straight to the native
// share sheet as a real URL.
referralShareBtn.addEventListener('click', async () => {
    referralShareBtn.disabled = true;
    try {
        const url = await core.invoke('get_or_create_referral_link');
        await core.invoke('plugin:share-sheet|share_text', { text: url });
    } catch (err) {
        setStatus(`Couldn't share: ${err}`);
    } finally {
        referralShareBtn.disabled = false;
    }
});

// ── Canonical profile ────────────────────────────────────────────────────────
//
// A separate, App-Group-shared record — independent of `cards`/`activeCard`
// above. Its own photo/field-list state is kept apart from the card editor's
// so editing this screen can never bleed into whichever card is currently
// being edited. Every field (built-in or pulled-in) is a {slug, name, value}
// tuple — the slug is the stable machine identity used for dedup, `name` is
// a human label, `value` is the content. Mirrors slugify() in
// src-tauri/src/lib.rs.

let preProfileView = 'cards'; // the view to return to on Close/Save
let pendingProfilePhoto = null;
let pendingProfileFields = [];
let editingProfileFieldIndex = null;

function slugify(s) {
    return s
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '_')
        .replace(/^_+|_+$/g, '');
}

function renderProfileFields() {
    profileFieldsEl.innerHTML = '';

    pendingProfileFields.forEach((entry, index) => {
        const li = document.createElement('li');
        li.className = 'link-entry';

        if (index === editingProfileFieldIndex) {
            const fields = document.createElement('div');
            fields.className = 'link-entry-edit-fields';

            const nameInput = document.createElement('input');
            nameInput.type = 'text';
            nameInput.placeholder = 'Field';
            nameInput.maxLength = 40;
            nameInput.value = entry.name;

            const valueInput = document.createElement('input');
            valueInput.type = 'text';
            valueInput.placeholder = 'Value';
            valueInput.value = entry.value;

            // Write through on every keystroke, so pendingProfileFields (what
            // Save sends) always matches what's on screen. Edits used to land
            // only on Enter or ✓, so fixing a value and tapping Save saved the
            // old one. Same fix as the card link editor.
            const writeThrough = () => {
                entry.name = nameInput.value.trim();
                entry.value = valueInput.value.trim();
                entry.slug = slugify(entry.name);
            };
            nameInput.addEventListener('input', writeThrough);
            valueInput.addEventListener('input', writeThrough);

            // Enter and ✓ only close the editor; the data is already in.
            const commit = () => {
                writeThrough();
                editingProfileFieldIndex = null;
                renderProfileFields();
            };
            // preventDefault: Enter in a form input is also an implicit Save.
            const onEnter = (e) => { if (e.key === 'Enter') { e.preventDefault(); commit(); } };
            nameInput.addEventListener('keydown', onEnter);
            valueInput.addEventListener('keydown', onEnter);

            fields.append(nameInput, valueInput);
            li.appendChild(fields);

            const doneBtn = document.createElement('button');
            doneBtn.type = 'button';
            doneBtn.textContent = '✓';
            doneBtn.addEventListener('click', commit);

            const actions = document.createElement('div');
            actions.className = 'link-entry-actions';
            actions.appendChild(doneBtn);
            li.appendChild(actions);

            profileFieldsEl.appendChild(li);
            nameInput.focus();
            return;
        }

        const text = document.createElement('div');
        text.className = 'link-entry-text';
        text.innerHTML = '<div class="link-entry-label"></div><div class="link-entry-url"></div>';
        text.querySelector('.link-entry-label').textContent = entry.name || entry.slug;
        text.querySelector('.link-entry-url').textContent = entry.value;
        text.addEventListener('click', () => {
            editingProfileFieldIndex = index;
            renderProfileFields();
        });
        li.appendChild(text);

        const actions = document.createElement('div');
        actions.className = 'link-entry-actions';

        const upBtn = document.createElement('button');
        upBtn.type = 'button';
        upBtn.textContent = '↑';
        upBtn.disabled = index === 0;
        upBtn.addEventListener('click', () => {
            [pendingProfileFields[index - 1], pendingProfileFields[index]] = [pendingProfileFields[index], pendingProfileFields[index - 1]];
            renderProfileFields();
        });

        const downBtn = document.createElement('button');
        downBtn.type = 'button';
        downBtn.textContent = '↓';
        downBtn.disabled = index === pendingProfileFields.length - 1;
        downBtn.addEventListener('click', () => {
            [pendingProfileFields[index], pendingProfileFields[index + 1]] = [pendingProfileFields[index + 1], pendingProfileFields[index]];
            renderProfileFields();
        });

        const removeBtn = document.createElement('button');
        removeBtn.type = 'button';
        removeBtn.textContent = '×';
        removeBtn.addEventListener('click', () => {
            pendingProfileFields.splice(index, 1);
            if (editingProfileFieldIndex === index) editingProfileFieldIndex = null;
            renderProfileFields();
        });

        actions.append(upBtn, downBtn, removeBtn);
        li.appendChild(actions);
        profileFieldsEl.appendChild(li);
    });

    const atLimit = pendingProfileFields.length >= MAX_PROFILE_FIELDS;
    profileAddFieldBtn.disabled = atLimit;
    profileFieldLimitHint.hidden = !atLimit;
}

// Moves whatever is typed in the "Add Field" row into pendingProfileFields.
// Shared by the Add button and Save — text left in the row used to be thrown
// away on Save, since Save only sends the list. Returns false (after telling
// the user why) when the row holds something that can't be added.
function takeNewProfileField() {
    const name = profileNewFieldName.value.trim();
    const value = profileNewFieldValue.value.trim();
    if (!name && !value) return true;
    if (!name || !value) {
        setStatus(name ? `Add a value for "${name}".` : 'Give the new field a name.');
        (name ? profileNewFieldValue : profileNewFieldName).focus();
        return false;
    }
    if (pendingProfileFields.length >= MAX_PROFILE_FIELDS) {
        setStatus(`Up to ${MAX_PROFILE_FIELDS} fields — remove one to add "${name}".`);
        return false;
    }

    pendingProfileFields.push({ slug: slugify(name), name, value });
    profileNewFieldName.value = '';
    profileNewFieldValue.value = '';
    renderProfileFields();
    return true;
}

profileAddFieldBtn.addEventListener('click', () => {
    if (!profileNewFieldName.value.trim() && !profileNewFieldValue.value.trim()) {
        setStatus('Type a field name and value first.');
        return;
    }
    takeNewProfileField();
});

// Enter in the Add row adds the field rather than submitting (saving) the form.
for (const input of [profileNewFieldName, profileNewFieldValue]) {
    input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); profileAddFieldBtn.click(); }
    });
}

profileChoosePhotoBtn.addEventListener('click', async () => {
    try {
        const path = await dialog.open({
            multiple: false,
            filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'heic'] }],
        });
        if (!path) return;

        const bytes = await fs.readFile(path);
        pendingProfilePhoto = await resizeImageToJpegBase64(bytes);
        setAvatarContent(profilePhotoPreview, pendingProfilePhoto, '');
    } catch (err) {
        setStatus(`Couldn't set photo: ${err}`);
    }
});

// Local-only: reads the `cards` array already loaded in memory (all of
// this app's own saved cards, not just the active one). First-non-empty-
// per-slug wins; never overwrites a field already present in the profile.
profilePullBtn.addEventListener('click', () => {
    if (cards.length === 0) {
        setStatus('No Linkitylink cards saved yet.');
        return;
    }
    const existingSlugs = new Set(pendingProfileFields.map((f) => f.slug));
    for (const c of cards) {
        if (c.name && !existingSlugs.has('name')) {
            pendingProfileFields.push({ slug: 'name', name: 'Name', value: c.name });
            existingSlugs.add('name');
        }
        if (c.bio && !existingSlugs.has('bio')) {
            pendingProfileFields.push({ slug: 'bio', name: 'Bio', value: c.bio });
            existingSlugs.add('bio');
        }
        for (const link of c.links || []) {
            const name = link.label || link.url;
            const slug = slugify(name);
            if (slug && !existingSlugs.has(slug)) {
                pendingProfileFields.push({ slug, name, value: link.url });
                existingSlugs.add(slug);
            }
        }
        if (c.photo && !pendingProfilePhoto) {
            pendingProfilePhoto = c.photo;
            setAvatarContent(profilePhotoPreview, pendingProfilePhoto, '');
        }
    }
    renderProfileFields();
    setStatus('Pulled in fields from your link cards.');
});

function fillProfileForm(profile) {
    pendingProfilePhoto = profile?.photo || null;
    pendingProfileFields = (profile?.fields || []).map((f) => ({ ...f }));
    editingProfileFieldIndex = null;
    // Save now adds whatever is left in the Add Field row, so text abandoned
    // on a previous visit (Close, not Save) must not be saved on this one.
    profileNewFieldName.value = '';
    profileNewFieldValue.value = '';
    setAvatarContent(profilePhotoPreview, profile?.photo, '');
    renderProfileFields();
}

function canonicalProfileFromForm() {
    return {
        photo: pendingProfilePhoto || undefined,
        fields: pendingProfileFields,
    };
}

function currentViewName() {
    if (!cardsView.hidden) return 'cards';
    if (!editView.hidden) return 'edit';
    if (!cardView.hidden) return 'card';
    return 'cards';
}

profileNavBtn.addEventListener('click', async () => {
    preProfileView = currentViewName();
    try {
        const profile = await core.invoke('load_canonical_profile');
        fillProfileForm(profile);
        showView('profile');
    } catch (err) {
        setStatus(`Couldn't load profile: ${err}`);
    }
});

profileForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (!takeNewProfileField()) return;
    try {
        await core.invoke('save_canonical_profile', { profile: canonicalProfileFromForm() });
        setStatus('Profile saved — shared across your apps.');
        showView(preProfileView);
    } catch (err) {
        setStatus(`Couldn't save: ${err}`);
    }
});

profileCloseBtn.addEventListener('click', () => showView(preProfileView));

async function init() {
    // Before loadCards, which renders the grid — the "Shared" badge needs
    // sharedCardId to already be populated on that first paint.
    await refreshSharedCardId();
    await loadCards();
    backgroundPublishAllCards();
}

init();
