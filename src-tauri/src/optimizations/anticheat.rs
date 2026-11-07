//! Anti-cheats: o AnalystBlaze nunca mexe neles.
//!
//! Vanguard (Riot), Easy Anti-Cheat, BattlEye, FACEIT, PunkBuster e ESEA são
//! drivers/serviços de kernel que o jogo exige. Parar um serviço, mudar a
//! prioridade de um processo ou apagar os arquivos de instalação/atualização
//! deles faz o jogo recusar abrir ("VAN", "EAC não carregou"). Esta lista é
//! a fonte única: serviços (safety::is_critical_service), processos
//! (protected_apps) e arquivos de temporários (cleanup) leem daqui.

/// Nomes de serviço (minúsculos).
pub const SERVICES: &[&str] = &[
    "vgc",
    "vgk",
    "easyanticheat",
    "easyanticheat_eos",
    "beservice",
    "bedaisy",
    "faceit",
    "pnkbstra",
    "pnkbstrb",
    "eseadriver2",
];

/// Executáveis (minúsculos) que ficam rodando enquanto o anti-cheat ou o
/// jogo que depende dele está aberto.
pub const PROCESSES: &[&str] = &[
    "vgc.exe",
    "vgtray.exe",
    "riotclientservices.exe",
    "riotclientux.exe",
    "riotclientuxrender.exe",
    "valorant-win64-shipping.exe",
    "easyanticheat.exe",
    "easyanticheat_eos.exe",
    "beservice.exe",
    "bedaisylauncher.exe",
    "faceitclient.exe",
    "faceit-ac.exe",
];

/// Arquivos cuja presença numa pasta de temporários indica instalação ou
/// atualização de anti-cheat em andamento (ou recém-feita): a pasta inteira
/// fica de fora da limpeza.
const FILE_MARKERS: &[&str] = &["vgc.exe", "vgk.sys", "vgtray.exe", "easyanticheat.exe", "easyanticheat_eos.exe", "beservice.exe", "bedaisy.sys"];

pub fn is_anticheat_service(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    SERVICES.contains(&normalized.as_str())
}

pub fn is_anticheat_process(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    PROCESSES.contains(&normalized.as_str())
}

pub fn is_anticheat_file_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    FILE_MARKERS.contains(&normalized.as_str()) || normalized.starts_with("easyanticheat")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanguard_service_and_driver_are_protected() {
        assert!(is_anticheat_service("vgc"));
        assert!(is_anticheat_service("VGK"));
        assert!(!is_anticheat_service("SysMain"));
    }

    #[test]
    fn riot_and_eac_processes_are_protected() {
        assert!(is_anticheat_process("VGC.exe"));
        assert!(is_anticheat_process("VALORANT-Win64-Shipping.exe"));
        assert!(is_anticheat_process("EasyAntiCheat_EOS.exe"));
        assert!(!is_anticheat_process("chrome.exe"));
    }

    #[test]
    fn installer_files_mark_a_temp_folder_as_off_limits() {
        assert!(is_anticheat_file_name("vgc.exe"));
        assert!(is_anticheat_file_name("EasyAntiCheat_Setup.exe"));
        assert!(!is_anticheat_file_name("installer.exe"));
    }
}
