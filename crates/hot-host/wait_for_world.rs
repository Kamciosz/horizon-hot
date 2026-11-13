// PL: Hak HEAD w `WorldRequest::prepare_world` (mixin `wait_for_world`, D11, S13). Silnik serwera,
// który trafi na świat z `session.lock` trzymanym przez inny silnik, czeka do 120 s, aż ten go
// zwolni. Stary silnik zwalnia blokadę dopiero po ostatnim zapisie (`save_game`, potem
// `save.flush()` w `stop_integrated`), więc nowy czyta świat kompletny. Po 120 s idzie dalej
// i `SessionLock::acquire` kończy się zwykłym błędem. Klienta gry hak nie dotyczy.
// EN: HEAD hook in `WorldRequest::prepare_world` (mixin `wait_for_world`, D11, S13). A server
// engine that meets a world whose `session.lock` another engine holds waits up to 120 s for it.
// The old engine releases the lock only after its last write (`save_game`, then `save.flush()`
// in `stop_integrated`), so the new one reads a complete world. After 120 s it goes on and
// `SessionLock::acquire` fails as usual. The game client is not affected.
fn wait_for_world(request: &WorldRequest, options: &Options) {
    let save_root = request.save_root.as_path();
    if options.dedicated.is_none() || !horizon_client::save::SessionLock::is_locked(save_root) {
        return;
    }
    // PL: Ten wiersz czyta `hotd` (i proxy z próby S13): silnik jest gotowy do podmiany.
    // EN: `hotd` (and the S13 proxy) read this line: the engine is ready for the swap.
    log::info!("horizon-hot: waiting for the world to be released by the running engine");
    let started = std::time::Instant::now();
    while horizon_client::save::SessionLock::is_locked(save_root) {
        if started.elapsed() > std::time::Duration::from_secs(120) {
            log::warn!("horizon-hot: the world is still in use after 120 s");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    log::info!(
        "horizon-hot: the world is free after {} ms of waiting",
        started.elapsed().as_millis()
    );
}
