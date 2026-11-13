// PL: Hak HEAD w `WorldRequest::prepare_world` (mixin `wait_for_world`, D11, S13). Silnik serwera,
// który trafi na świat z `session.lock` trzymanym przez inny silnik, czeka do 120 s, aż ten go
// zwolni. Stary silnik serwera zwalnia blokadę dopiero po `stop_dedicated` (zapis graczy
// i świata), gdy zwalnia `Game`, więc nowy czyta świat kompletny. Po 120 s albo po SIGTERM
// (`systemctl stop`, przerwana podmiana) idzie dalej i `SessionLock::acquire` kończy się zwykłym
// błędem. Supervisor wysyła `stop` na stdin, którego czekający silnik nie czyta, więc samo
// `horizon-server stop` czeka 120 s. Klienta gry hak nie dotyczy.
// EN: HEAD hook in `WorldRequest::prepare_world` (mixin `wait_for_world`, D11, S13). A server
// engine that meets a world whose `session.lock` another engine holds waits up to 120 s for it.
// The old server engine releases the lock only after `stop_dedicated` (players and world saved),
// when it drops `Game`, so the new one reads a complete world. After 120 s or on SIGTERM
// (`systemctl stop`, an aborted swap) it goes on and `SessionLock::acquire` fails as usual. The
// supervisor sends `stop` on stdin, which a waiting engine does not read, so `horizon-server stop`
// alone waits 120 s. The game client is not affected.
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
        if horizon_threads::stop_requests() > 0 {
            log::info!("horizon-hot: stop requested while waiting for the world");
            return;
        }
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
