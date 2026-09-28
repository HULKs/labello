// Trunk publishes content-hashed JS/WASM URLs. Refresh the entry document and
// its preload inventory before navigating; never clear account or draft storage.
export async function prepareBuildReload(target, manual) {
    const key = `labello:build-reload:${location.pathname}`;
    try {
        if (!manual && sessionStorage.getItem(key) === target) {
            throw new Error('already-attempted');
        }
        sessionStorage.setItem(key, target);
    } catch (error) {
        if (error.message === 'already-attempted') {
            throw new Error('The builds still differ after an update attempt. Retry when deployment is complete.');
        }
        throw new Error('Automatic update could not access tab storage. Allow browser storage and retry.');
    }
    try {
        // Labello has no service worker. Do not claim a cache bypass if an
        // externally installed worker can intercept these requests.
        if (navigator.serviceWorker?.controller) throw new Error('worker');
        const destination = new URL(location.href);
        destination.searchParams.set('labello-update', crypto.randomUUID());
        const options = {cache: 'reload', credentials: 'same-origin', signal: AbortSignal.timeout(30000)};
        const entry = await fetch(destination, options);
        if (!entry.ok || !entry.headers.get('content-type')?.includes('text/html')) throw new Error('entry');
        const document = new DOMParser().parseFromString(await entry.text(), 'text/html');
        const assets = [...document.querySelectorAll('link[rel="modulepreload"], link[rel="preload"][as="fetch"], script[src]')]
            .map(node => new URL(node.getAttribute('href') || node.getAttribute('src'), destination));
        if (!assets.some(url => url.pathname.endsWith('.wasm')) || !assets.some(url => url.pathname.endsWith('.js'))) {
            throw new Error('inventory');
        }
        await Promise.all(assets.map(async url => {
            if (url.origin !== destination.origin) throw new Error('origin');
            const response = await fetch(url, options);
            if (!response.ok) throw new Error('asset');
            await response.arrayBuffer();
        }));
        return destination.href;
    } catch (_) {
        throw new Error('The app update could not load fresh assets. Check the connection or deployment, then retry.');
    }
}
