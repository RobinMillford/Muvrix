use eframe::egui::{self, Color32, RichText};
use muvrix_core::network::{
    redact_credentials_from_url, JellyfinProvider, NetworkConnection, NetworkProvider, NfsProvider,
    NfsVersion, PlexProvider, SftpProvider, SmbProvider, WebDavProvider,
};

use crate::app::{format_timestamp, MuvrixApp};
use crate::remote_playback::build_remote_stream_with_telemetry;
use crate::ui_components::BrandPalette;
use crate::views::{format_file_size, AppView};

impl MuvrixApp {
    pub(crate) fn render_settings_network(&mut self, ui: &mut egui::Ui) {
        // Section 5: Network Media Connections (WebDAV)
        ui.group(|ui| {
                        ui.label(
                            RichText::new("5. Network Media Connections & Streaming")
                                .strong()
                                .size(14.0)
                                .color(Color32::from_rgb(95, 185, 255)),
                        );
                        ui.label(
                            RichText::new(
                                "Configure WebDAV / HTTP endpoints for direct network streaming. Credentials are held in application memory for this session and are not persisted by Muvrix.",
                            )
                            .size(12.0)
                            .color(Color32::from_gray(160)),
                        );

                        ui.add_space(8.0);

                        let connections = self.db.list_connections().unwrap_or_default();
                        let mut delete_conn_id: Option<i64> = None;

                        if connections.is_empty() {
                            ui.label(RichText::new("No network connections configured.").italics().color(Color32::from_gray(140)));
                        } else {
                            let mut stream_target: Option<(NetworkConnection, String)> = None;
                            let mut browse_toggle: Option<(i64, String)> = None;

                            for conn in &connections {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            let proto_str = conn.protocol.to_lowercase();
                                            let (badge_text, badge_color) = match proto_str.as_str() {
                                                "webdav" | "http" | "https" => ("[WEBDAV]", Color32::from_rgb(125, 211, 252)),
                                                "smb" => ("[SMB2/3]", Color32::from_rgb(74, 222, 128)),
                                                "sftp" => ("[SFTP]", Color32::from_rgb(192, 132, 252)),
                                                "nfs" => ("[NFSv4]", Color32::from_rgb(244, 114, 182)),
                                                "jellyfin" => ("[JELLYFIN]", Color32::from_rgb(167, 139, 250)),
                                                "plex" => ("[PLEX]", Color32::from_rgb(251, 191, 36)),
                                                _ => ("[REMOTE]", Color32::from_rgb(251, 191, 36)),
                                            };

                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(badge_text)
                                                        .monospace()
                                                        .size(11.0)
                                                        .color(badge_color),
                                                );
                                                ui.label(RichText::new(&conn.name).strong());
                                            });
                                            let raw_url = format!("{}://{}:{}{}", conn.protocol, conn.host, conn.port, conn.remote_path);
                                            let redacted = redact_credentials_from_url(&raw_url);
                                            ui.label(RichText::new(redacted).size(11.0).color(Color32::from_gray(150)));
                                        });

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.button("Delete").clicked() {
                                                delete_conn_id = Some(conn.id);
                                            }
                                            if ui.button("▶ Stream").on_hover_text("Open network stream directly in Muvrix player").clicked() {
                                                stream_target = Some((conn.clone(), conn.remote_path.clone()));
                                            }
                                            let is_browsing = self.browsing_conn_id == Some(conn.id);
                                            let b_label = if is_browsing { "Close Browser ✖" } else { "Browse 📂" };
                                            if ui.button(b_label).clicked() {
                                                browse_toggle = Some((conn.id, conn.remote_path.clone()));
                                            }
                                        });
                                    });

                                    // Render Remote Media Browser when active for this connection
                                    if self.browsing_conn_id == Some(conn.id) {
                                        ui.add_space(4.0);
                                        ui.separator();
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new("Path:").monospace().color(Color32::from_gray(140)));
                                            ui.label(RichText::new(&self.browsing_path).monospace().color(Color32::WHITE));

                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if ui.button("🔄 Refresh").clicked() {
                                                    self.refresh_remote_directory(conn);
                                                }
                                                if self.browsing_path != "/" && ui.button("⬆ Up").clicked() {
                                                    if let Some(idx) = self.browsing_path.trim_end_matches('/').rfind('/') {
                                                        let parent = &self.browsing_path[..idx];
                                                        self.browsing_path = if parent.is_empty() { "/".to_string() } else { parent.to_string() };
                                                    } else {
                                                        self.browsing_path = "/".to_string();
                                                    }
                                                    self.refresh_remote_directory(conn);
                                                }
                                            });
                                        });

                                        if self.browsing_loading {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label(RichText::new("Loading directory...").italics().color(Color32::from_gray(160)));
                                            });
                                        } else if let Some(ref err) = self.browsing_error {
                                            ui.label(RichText::new(format!("⚠ {}", err)).color(Color32::from_rgb(255, 120, 120)).size(11.5));
                                        } else if self.browsing_entries.is_empty() {
                                            ui.label(RichText::new("No entries found in this directory.").italics().color(Color32::from_gray(140)));
                                        } else {
                                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                                for entry in &self.browsing_entries.clone() {
                                                    ui.horizontal(|ui| {
                                                        if entry.is_dir {
                                                            if ui.button(format!("📁 {}", entry.name)).clicked() {
                                                                self.browsing_path = entry.remote_path.clone();
                                                                self.refresh_remote_directory(conn);
                                                            }
                                                        } else {
                                                            ui.label(format!("🎬 {}", entry.name));
                                                            ui.label(RichText::new(format_file_size(entry.size_bytes)).size(10.5).color(Color32::from_gray(140)));

                                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                                if ui.button("▶ Play").clicked() {
                                                                    stream_target = Some((conn.clone(), entry.remote_path.clone()));
                                                                }
                                                            });
                                                        }
                                                    });
                                                }
                                            });
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }

                            if let Some((id, path)) = browse_toggle {
                                if self.browsing_conn_id == Some(id) {
                                    self.browsing_conn_id = None;
                                } else {
                                    self.browsing_conn_id = Some(id);
                                    if let Some(conn) = connections.iter().find(|c| c.id == id) {
                                        self.browsing_path = if conn.protocol.eq_ignore_ascii_case("smb")
                                        {
                                            // The share is carried in conn.remote_path;
                                            // browsing starts at the share root.
                                            "/".to_string()
                                        } else if path.is_empty() {
                                            "/".to_string()
                                        } else {
                                            path
                                        };
                                        self.refresh_remote_directory(conn);
                                    }
                                }
                            }

                            if let Some((conn, target)) = stream_target {
                                let telemetry = muvrix_player::StreamTelemetry::new();
                                self.controller.set_stream_telemetry(Some(telemetry.clone()));
                                let policy = muvrix_player::StreamRetryPolicy {
                                    max_retries: self.stream_max_retries,
                                    ..Default::default()
                                };
                                match build_remote_stream_with_telemetry(
                                    &conn,
                                    &target,
                                    &self.db,
                                    &self.network_vault,
                                    policy,
                                    Some(telemetry),
                                ) {
                                    Ok(stream) => {
                                        let uri = self.stream_bridge.register(stream);
                                        self.browsing_error = None;
                                        self.load_file(&uri);
                                        self.current_view = AppView::Player;
                                    }
                                    Err(e) => {
                                        let message = format!("Remote playback failed: {e}");
                                        self.browsing_error = Some(message.clone());
                                        self.status_message = message;
                                    }
                                }
                            }
                        }

                        if let Some(id) = delete_conn_id {
                            if let Some(conn) = connections.iter().find(|c| c.id == id) {
                                if let Some(ref cred_ref) = conn.credential_ref {
                                    let _ = self.network_vault.remove_native(cred_ref);
                                }
                            }
                            let _ = self.db.delete_connection(id);
                        }

                        // Trusted SSH Host Fingerprints (TOFU Database)
                        let trusted_hosts = self.db.list_ssh_host_trust().unwrap_or_default();
                        if !trusted_hosts.is_empty() {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.label(RichText::new("Trusted SSH Host Fingerprints (TOFU Database):").strong().size(12.5));
                            let mut revoke_target: Option<(String, u16)> = None;
                            egui::Grid::new("ssh_trusted_hosts_grid").striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                                ui.label(RichText::new("Host:Port").strong());
                                ui.label(RichText::new("Algorithm").strong());
                                ui.label(RichText::new("Fingerprint (SHA256)").strong());
                                ui.label(RichText::new("Action").strong());
                                ui.end_row();

                                for trust in &trusted_hosts {
                                    ui.label(format!("{}:{}", trust.host, trust.port));
                                    ui.label(&trust.key_algorithm);
                                    ui.label(RichText::new(&trust.fingerprint_sha256).monospace().size(11.0));
                                    if ui.button("Revoke").clicked() {
                                        revoke_target = Some((trust.host.clone(), trust.port));
                                    }
                                    ui.end_row();
                                }
                            });
                            if let Some((h, p)) = revoke_target {
                                let _ = self.db.revoke_ssh_host_trust(&h, p);
                                self.network_status_msg = Some("SSH host fingerprint trust revoked.".to_string());
                            }
                        }

                        ui.add_space(6.0);

                        let add_label = if self.add_network_open { "Hide Add Connection Form" } else { "➕ Add Network Connection" };
                        if ui.button(add_label).clicked() {
                            self.add_network_open = !self.add_network_open;
                        }

                        if self.add_network_open {
                            ui.add_space(6.0);
                            let form_group = ui.group(|ui| {
                                ui.label(RichText::new("New Network Remote Endpoint").strong().size(12.5));

                                ui.horizontal(|ui| {
                                    ui.label("Protocol:   ");
                                    let cur_proto = self.network_protocol_input.clone();
                                    let proto_label = match cur_proto.as_str() {
                                        "smb" => "SMB / Samba Share (Windows / NAS)",
                                        "sftp" => "SFTP (SSH File Transfer Protocol)",
                                        "nfs" => "NFS (Network File System v4 / Unix)",
                                        "jellyfin" => "Jellyfin / Emby Media Server",
                                        "plex" => "Plex Media Server",
                                        _ => "WebDAV / Nextcloud / ownCloud",
                                    };
                                    egui::ComboBox::from_id_salt("net_proto_combo")
                                        .selected_text(proto_label)
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_label(self.network_protocol_input == "webdav", "WebDAV / Nextcloud").clicked() {
                                                self.network_protocol_input = "webdav".to_string();
                                                self.network_port_input = if self.network_https_input { 443 } else { 80 };
                                            }
                                            if ui.selectable_label(self.network_protocol_input == "smb", "SMB / Samba Share").clicked() {
                                                self.network_protocol_input = "smb".to_string();
                                                self.network_port_input = 445;
                                            }
                                            if ui.selectable_label(self.network_protocol_input == "sftp", "SFTP (SSH File Transfer)").clicked() {
                                                self.network_protocol_input = "sftp".to_string();
                                                self.network_port_input = 22;
                                            }
                                            if ui.selectable_label(self.network_protocol_input == "nfs", "NFS (Network File System v4)").clicked() {
                                                self.network_protocol_input = "nfs".to_string();
                                                self.network_port_input = 2049;
                                            }
                                            if ui.selectable_label(self.network_protocol_input == "jellyfin", "Jellyfin / Emby Media Server").clicked() {
                                                self.network_protocol_input = "jellyfin".to_string();
                                                self.network_port_input = if self.network_https_input { 8920 } else { 8096 };
                                            }
                                            if ui.selectable_label(self.network_protocol_input == "plex", "Plex Media Server").clicked() {
                                                self.network_protocol_input = "plex".to_string();
                                                self.network_port_input = 32400;
                                            }
                                        });
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Name:       ");
                                    ui.add(egui::TextEdit::singleline(&mut self.network_name_input).hint_text("My NAS / Server"));
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Host / IP:  ");
                                    ui.add(egui::TextEdit::singleline(&mut self.network_host_input).hint_text("nas.local or 192.168.1.100"));
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Port:       ");
                                    ui.add(egui::DragValue::new(&mut self.network_port_input).range(1..=65535));
                                    if (self.network_protocol_input == "webdav" || self.network_protocol_input == "jellyfin" || self.network_protocol_input == "plex")
                                        && ui
                                            .checkbox(&mut self.network_https_input, "Use HTTPS")
                                            .changed()
                                    {
                                        if self.network_protocol_input == "webdav" {
                                            self.network_port_input = if self.network_https_input { 443 } else { 80 };
                                        } else if self.network_protocol_input == "jellyfin" {
                                            self.network_port_input = if self.network_https_input { 8920 } else { 8096 };
                                        }
                                    }
                                });

                                ui.horizontal(|ui| {
                                    let path_label = match self.network_protocol_input.as_str() {
                                        "nfs" => "Export Path: ",
                                        "jellyfin" | "plex" => "Library Path:",
                                        _ => "Path/Share: ",
                                    };
                                    ui.label(path_label);
                                    let hint = match self.network_protocol_input.as_str() {
                                        "smb" => "ShareName/Movies/video.mp4",
                                        "sftp" => "/home/user/media/video.mkv",
                                        "nfs" => "/volume1/video or /export/media",
                                        "jellyfin" => "/ (all libraries) or /items/{id}",
                                        "plex" => "/ (all sections) or /library/sections/{id}",
                                        _ => "/remote.php/webdav/Media/",
                                    };
                                    ui.add(egui::TextEdit::singleline(&mut self.network_path_input).hint_text(hint));
                                });

                                if self.network_protocol_input == "nfs" {
                                    ui.horizontal(|ui| {
                                        ui.label("NFS Version: ");
                                        ui.radio_value(&mut self.network_nfs_version, NfsVersion::V4, "NFSv4 (Default)");
                                        ui.radio_value(&mut self.network_nfs_version, NfsVersion::V3, "NFSv3 (Legacy)");
                                    });
                                }

                                if self.network_protocol_input == "sftp" {
                                    ui.horizontal(|ui| {
                                        ui.label("Host Key:   ");
                                        ui.add(egui::TextEdit::singleline(&mut self.network_host_key_input).hint_text("SHA256:... (pinned server fingerprint)"));
                                        if ui.button("Probe Host Key").clicked() && !self.network_host_input.trim().is_empty() {
                                            match SftpProvider::inspect_host_key(&self.network_host_input, self.network_port_input) {
                                                Ok(presented) => {
                                                    self.pending_ssh_host_trust = Some(presented);
                                                    self.ssh_host_trust_error = None;
                                                }
                                                Err(e) => {
                                                    self.ssh_host_trust_error = Some(format!("Failed to probe host key: {e}"));
                                                }
                                            }
                                        }
                                    });

                                    if let Some(presented) = self.pending_ssh_host_trust.clone() {
                                        let mut clear_trust_prompt = false;
                                        let mut approved = false;
                                        ui.group(|ui| {
                                            ui.label(RichText::new("⚠️ SSH Host Key Verification (Trust On First Use)").strong().color(Color32::from_rgb(251, 191, 36)));
                                            ui.label(format!("Endpoint: {}:{}", presented.host, presented.port));
                                            ui.label(format!("Key Type: {}", presented.algorithm));
                                            ui.label(RichText::new(format!("SHA-256 Fingerprint: {}", presented.fingerprint)).monospace().strong());
                                            ui.label(RichText::new("CRITICAL: Verify this fingerprint matches your server's host key using a trusted out-of-band channel (e.g. server console, physical terminal, or verified administrator call). Never approve an unexpected fingerprint. Approving stores persistent trust in the local database.").size(11.0).color(Color32::from_rgb(240, 200, 120)));

                                            ui.horizontal(|ui| {
                                                if ui.button(RichText::new("✓ Approve & Trust Host").strong().color(Color32::from_rgb(74, 222, 128))).clicked() {
                                                    approved = true;
                                                    clear_trust_prompt = true;
                                                }
                                                if ui.button("✗ Reject").clicked() {
                                                    clear_trust_prompt = true;
                                                    self.network_status_msg = Some("SSH Host verification rejected by user.".to_string());
                                                }
                                            });
                                        });
                                        if approved {
                                            let _ = self.db.record_ssh_host_trust(
                                                &presented.host,
                                                presented.port,
                                                &presented.algorithm,
                                                &presented.fingerprint,
                                                None,
                                            );
                                            self.network_host_key_input = presented.fingerprint.clone();
                                            self.network_status_msg = Some(format!("SSH Host {}:{} trusted in database.", presented.host, presented.port));
                                        }
                                        if clear_trust_prompt {
                                            self.pending_ssh_host_trust = None;
                                        }
                                    }

                                    if let Some(ref err) = self.ssh_host_trust_error {
                                        ui.label(RichText::new(format!("⚠️ {}", err)).color(Color32::from_rgb(255, 120, 120)).size(11.5));
                                    }
                                }

                                ui.horizontal(|ui| {
                                    let user_label = match self.network_protocol_input.as_str() {
                                        "jellyfin" => "User ID (opt):",
                                        "nfs" => "Unix User:  ",
                                        _ => "Username:   ",
                                    };
                                    ui.label(user_label);
                                    ui.add(egui::TextEdit::singleline(&mut self.network_user_input));
                                });

                                ui.horizontal(|ui| {
                                    let pass_label = match self.network_protocol_input.as_str() {
                                        "jellyfin" | "plex" => "API Token:  ",
                                        "nfs" => "Secret (opt):",
                                        _ => "Password:   ",
                                    };
                                    ui.label(pass_label);
                                    ui.add(egui::TextEdit::singleline(&mut self.network_pass_input).password(true));
                                });

                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    if ui.button(RichText::new("Save Connection").strong()).clicked()
                                        && !self.network_host_input.trim().is_empty()
                                    {
                                        let proto = self.network_protocol_input.clone();
                                        // Persist the HTTPS checkbox for WebDAV so a
                                        // non-standard port does not get treated as HTTP.
                                        let connection_protocol = if proto == "webdav"
                                            && self.network_https_input
                                        {
                                            "https".to_string()
                                        } else {
                                            proto.clone()
                                        };
                                        let cred_ref = if !self.network_pass_input.is_empty() {
                                            let key = muvrix_core::network::generate_credential_ref(
                                                &proto,
                                                self.network_host_input.trim(),
                                                self.network_port_input,
                                                self.network_user_input.trim(),
                                            );
                                            match self.network_vault.store_native(&key, &self.network_pass_input) {
                                                Ok(()) => Some(key),
                                                Err(e) => {
                                                    self.network_status_msg = Some(format!("Failed to save credential to secure storage: {e}"));
                                                    return;
                                                }
                                            }
                                        } else {
                                            None
                                        };
                                        let conn = NetworkConnection {
                                            id: 0,
                                            name: if self.network_name_input.trim().is_empty() {
                                                format!("{} ({})", proto.to_uppercase(), self.network_host_input)
                                            } else {
                                                self.network_name_input.trim().to_string()
                                            },
                                            protocol: connection_protocol,
                                            host: self.network_host_input.trim().to_string(),
                                            port: self.network_port_input,
                                            remote_path: if self.network_path_input.trim().is_empty() {
                                                "/".to_string()
                                            } else {
                                                self.network_path_input.trim().to_string()
                                            },
                                            auth_type: if self.network_pass_input.is_empty() { "anonymous".to_string() } else { "basic".to_string() },
                                            credential_ref: cred_ref,
                                            is_anonymous: self.network_pass_input.is_empty(),
                                            auto_connect: false,
                                            last_connected_at: 0,
                                        };
                                        if let Ok(Some(existing)) = self.db.find_connection_by_endpoint(
                                            &conn.protocol,
                                            &conn.host,
                                            conn.port,
                                            &conn.remote_path,
                                        ) {
                                            self.network_status_msg = Some(format!(
                                                "Connection already exists (ID {}: {}). Duplicate prevented.",
                                                existing.id, existing.name
                                            ));
                                        } else {
                                            let _ = self.db.insert_connection(&conn);
                                            self.network_status_msg = Some("Connection saved successfully".to_string());
                                            self.add_network_open = false;
                                        }
                                    }

                                    if ui.button("Test Endpoint").clicked() {
                                        match self.network_protocol_input.as_str() {
                                            "sftp" => {
                                                let fp = if self.network_host_key_input.trim().is_empty() {
                                                    None
                                                } else {
                                                    Some(self.network_host_key_input.trim().to_string())
                                                };
                                                let provider = SftpProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    &self.network_path_input,
                                                    &self.network_user_input,
                                                    if self.network_pass_input.is_empty() { None } else {
                                                        let key = muvrix_core::network::generate_credential_ref(
                                                            "sftp",
                                                            self.network_host_input.trim(),
                                                            self.network_port_input,
                                                            self.network_user_input.trim(),
                                                        );
                                                        self.network_vault.store_ephemeral(&key, &self.network_pass_input);
                                                        Some(key)
                                                    },
                                                    None,
                                                    fp,
                                                    self.network_vault.clone(),
                                                ).with_database(self.db.clone());
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: SFTP endpoint {}:{} verified with host-key security", self.network_host_input, self.network_port_input));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("SFTP Error: {}", e));
                                                    }
                                                }
                                            }
                                            "smb" => {
                                                let provider = SmbProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    if self.network_path_input.is_empty() { "Share" } else { &self.network_path_input },
                                                    "/",
                                                    if self.network_user_input.is_empty() { None } else { Some(self.network_user_input.clone()) },
                                                    None,
                                                    if self.network_pass_input.is_empty() { None } else {
                                                        let key = muvrix_core::network::generate_credential_ref(
                                                            "smb",
                                                            self.network_host_input.trim(),
                                                            self.network_port_input,
                                                            self.network_user_input.trim(),
                                                        );
                                                        self.network_vault.store_ephemeral(&key, &self.network_pass_input);
                                                        Some(key)
                                                    },
                                                    self.network_pass_input.is_empty(),
                                                    self.network_vault.clone(),
                                                );
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: SMB share on {} verified", self.network_host_input));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("SMB Error: {}", e));
                                                    }
                                                }
                                            }
                                            "nfs" => {
                                                let provider = NfsProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    if self.network_path_input.is_empty() { "/" } else { &self.network_path_input },
                                                    self.network_nfs_version,
                                                    if self.network_user_input.is_empty() { None } else { Some(self.network_user_input.clone()) },
                                                    None,
                                                    self.network_vault.clone(),
                                                );
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: {} export on {}:{} verified via RPC NULL probe", self.network_nfs_version, self.network_host_input, self.network_port_input));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("NFS Error: {}", e));
                                                    }
                                                }
                                            }
                                            "jellyfin" => {
                                                let provider = JellyfinProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    self.network_https_input || self.network_port_input == 8920,
                                                    if self.network_pass_input.is_empty() { None } else { Some(self.network_pass_input.clone()) },
                                                    None,
                                                    if self.network_user_input.is_empty() { None } else { Some(self.network_user_input.clone()) },
                                                    self.network_vault.clone(),
                                                );
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: Jellyfin server at {} verified", provider.base_url()));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("Jellyfin Error: {}", e));
                                                    }
                                                }
                                            }
                                            "plex" => {
                                                let provider = PlexProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    self.network_https_input || self.network_port_input == 443,
                                                    if self.network_pass_input.is_empty() { None } else { Some(self.network_pass_input.clone()) },
                                                    None,
                                                    self.network_vault.clone(),
                                                );
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: Plex Media Server at {} verified", provider.base_url()));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("Plex Error: {}", e));
                                                    }
                                                }
                                            }
                                            _ => {
                                                let provider = WebDavProvider::new(
                                                    &self.network_host_input,
                                                    self.network_port_input,
                                                    &self.network_path_input,
                                                    self.network_https_input,
                                                    if self.network_user_input.is_empty() { None } else { Some(self.network_user_input.clone()) },
                                                    if self.network_pass_input.is_empty() { None } else {
                                                        let key = muvrix_core::network::generate_credential_ref(
                                                            "webdav",
                                                            self.network_host_input.trim(),
                                                            self.network_port_input,
                                                            self.network_user_input.trim(),
                                                        );
                                                        self.network_vault.store_ephemeral(&key, &self.network_pass_input);
                                                        Some(key)
                                                    },
                                                    self.network_vault.clone(),
                                                );
                                                match provider.test_connection() {
                                                    Ok(true) => {
                                                        self.network_status_msg = Some(format!("Success: WebDAV endpoint {} verified", provider.base_url()));
                                                    }
                                                    Ok(false) => {
                                                        self.network_status_msg = Some("Connection test returned negative".to_string());
                                                    }
                                                    Err(e) => {
                                                        self.network_status_msg = Some(format!("WebDAV Error: {}", e));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                });
                            })
                            .response;
                            if std::env::var("MUVRIX_SCROLL_TO_FORM").is_ok() {
                                form_group.scroll_to_me(Some(egui::Align::Center));
                            }
                        }

                        if let Some(ref msg) = self.network_status_msg {
                            ui.add_space(4.0);
                            ui.label(RichText::new(msg).color(Color32::from_rgb(100, 200, 100)).size(11.5));
                        }
                    });

        // Section 5.1: Network Streaming Hardening, Buffering & Telemetry
        ui.add_space(10.0);
        ui.group(|ui| {
            ui.label(
                RichText::new("5.1 Streaming Hardening, Demuxer Cache & Auto-Reconnect")
                    .strong()
                    .size(14.0)
                    .color(Color32::from_rgb(95, 185, 255)),
            );
            ui.label(
                RichText::new(
                    "Configure forward stream buffering, backwards seek cache, read-ahead window, and transient reconnect policies for remote media (WebDAV, SMB, SFTP).",
                )
                .size(12.0)
                .color(Color32::from_gray(160)),
            );

            ui.add_space(8.0);

            let mut changed = false;

            // Stream Buffer Size (Forward Cache)
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Stream Buffer Size:")
                        .strong()
                        .color(Color32::from_gray(210)),
                );
                let slider = egui::Slider::new(&mut self.stream_buffer_mb, 50..=500)
                    .suffix(" MiB")
                    .text("Forward Demuxer Cache");
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            // Back-Buffer Size (Reverse Seek Cache)
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Back-Buffer Size:")
                        .strong()
                        .color(Color32::from_gray(210)),
                );
                let slider = egui::Slider::new(&mut self.stream_back_buffer_mb, 20..=150)
                    .suffix(" MiB")
                    .text("Reverse Seek Memory");
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            // Read-Ahead Window
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Read-Ahead Window:")
                        .strong()
                        .color(Color32::from_gray(210)),
                );
                let slider = egui::Slider::new(&mut self.stream_readahead_secs, 5..=60)
                    .suffix(" s")
                    .text("Demuxer Prefetch Time");
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            // Max Reconnect Attempts
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Max Reconnect Retries:")
                        .strong()
                        .color(Color32::from_gray(210)),
                );
                let slider = egui::Slider::new(&mut self.stream_max_retries, 1..=10)
                    .text("Attempts Before Failure");
                if ui.add(slider).changed() {
                    changed = true;
                }
            });

            if changed {
                self.apply_stream_buffering_settings();
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(6.0);

            // Live Demuxer & Stream Telemetry Status Card
            ui.label(
                RichText::new("Live Stream & Demuxer Telemetry")
                    .strong()
                    .size(12.5)
                    .color(BrandPalette::ELECTRIC_LIME),
            );

            let cache_duration = self.controller.demuxer_cache_duration();
            let cache_time = self.controller.demuxer_cache_time();
            let is_idle = self.controller.demuxer_cache_idle();
            let is_buffering = self.controller.is_paused_for_cache();

            let demuxer_status = if is_buffering {
                ("Buffering (Stalled)", Color32::from_rgb(255, 180, 50))
            } else if is_idle {
                ("Idle (Buffer Full)", Color32::from_rgb(140, 210, 140))
            } else {
                ("Active (Prefetching)", Color32::from_rgb(90, 180, 255))
            };

            egui::Grid::new("stream_telemetry_grid")
                .num_columns(2)
                .spacing([20.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Demuxer Status:").color(Color32::from_gray(160)));
                    ui.label(RichText::new(demuxer_status.0).strong().color(demuxer_status.1));
                    ui.end_row();

                    ui.label(RichText::new("Cached Duration:").color(Color32::from_gray(160)));
                    ui.label(
                        RichText::new(format!("{:.1} s ahead of playhead", cache_duration))
                            .monospace()
                            .color(Color32::WHITE),
                    );
                    ui.end_row();

                    ui.label(RichText::new("Buffer End Position:").color(Color32::from_gray(160)));
                    ui.label(
                        RichText::new(format_timestamp(cache_time))
                            .monospace()
                            .color(Color32::from_rgb(125, 211, 252)),
                    );
                    ui.end_row();

                    if let Some(telemetry) = self.controller.stream_telemetry() {
                        let state = telemetry.current_state();
                        let (state_text, state_col) = match state {
                            muvrix_player::RecoveryState::Connected => (
                                "Connected & Streaming".to_string(),
                                Color32::from_rgb(100, 220, 100),
                            ),
                            muvrix_player::RecoveryState::Reconnecting {
                                attempt,
                                max_attempts,
                            } => (
                                format!("⚡ Reconnecting ({attempt}/{max_attempts})"),
                                Color32::from_rgb(255, 180, 40),
                            ),
                            muvrix_player::RecoveryState::Failed(msg) => {
                                (format!("Failed: {msg}"), Color32::from_rgb(255, 80, 80))
                            }
                            muvrix_player::RecoveryState::Closed => {
                                ("Closed".to_string(), Color32::from_gray(140))
                            }
                        };

                        ui.label(RichText::new("Transport State:").color(Color32::from_gray(160)));
                        ui.label(RichText::new(state_text).strong().color(state_col));
                        ui.end_row();

                        ui.label(RichText::new("Bytes Streamed:").color(Color32::from_gray(160)));
                        ui.label(
                            RichText::new(format_file_size(telemetry.bytes_read()))
                                .monospace()
                                .color(Color32::WHITE),
                        );
                        ui.end_row();

                        ui.label(RichText::new("Reconnect Attempts:").color(Color32::from_gray(160)));
                        ui.label(
                            RichText::new(format!("{}", telemetry.reconnect_attempts()))
                                .monospace()
                                .color(if telemetry.reconnect_attempts() > 0 {
                                    Color32::from_rgb(255, 200, 80)
                                } else {
                                    Color32::WHITE
                                }),
                        );
                        ui.end_row();
                    } else {
                        ui.label(RichText::new("Transport Stream:").color(Color32::from_gray(160)));
                        ui.label(
                            RichText::new("Local file playback / no custom stream mounted")
                                .italics()
                                .color(Color32::from_gray(140)),
                        );
                        ui.end_row();
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            // Connection Pool & WAN Keep-Alive Status Card
            let pool_card = ui
                .group(|ui| {
                    ui.label(
                        RichText::new("Connection Pool & WAN Keep-Alive Monitor")
                            .strong()
                            .size(12.5)
                            .color(Color32::from_rgb(140, 230, 120)),
                    );

                    let pool_telemetry = self.connection_pool.telemetry();

                    egui::Grid::new("connection_pool_telemetry_grid")
                        .num_columns(2)
                        .spacing([20.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Active Sockets:").color(Color32::from_gray(160)));
                            ui.label(
                                RichText::new(format!("{}", pool_telemetry.active_connections))
                                    .monospace()
                                    .color(Color32::WHITE),
                            );
                            ui.end_row();

                            ui.label(RichText::new("Pooled Idle Sockets:").color(Color32::from_gray(160)));
                            ui.label(
                                RichText::new(format!("{}", pool_telemetry.idle_connections))
                                    .monospace()
                                    .color(Color32::from_rgb(125, 211, 252)),
                            );
                            ui.end_row();

                            ui.label(RichText::new("Recycled Connections:").color(Color32::from_gray(160)));
                            ui.label(
                                RichText::new(format!("{}", pool_telemetry.recycled_count))
                                    .monospace()
                                    .color(Color32::WHITE),
                            );
                            ui.end_row();

                            ui.label(RichText::new("Keep-Alive Probes Sent:").color(Color32::from_gray(160)));
                            ui.label(
                                RichText::new(format!(
                                    "{}/{} successful",
                                    pool_telemetry.probes_successful, pool_telemetry.probes_sent
                                ))
                                .monospace()
                                .color(Color32::from_rgb(100, 220, 100)),
                            );
                            ui.end_row();

                            ui.label(RichText::new("Socket Reuses:").color(Color32::from_gray(160)));
                            ui.label(
                                RichText::new(format!("{}", pool_telemetry.socket_reuse_count))
                                    .monospace()
                                    .color(BrandPalette::ELECTRIC_LIME),
                            );
                            ui.end_row();

                            ui.label(RichText::new("Average Link Latency:").color(Color32::from_gray(160)));
                            let latency_color = if pool_telemetry.average_rtt_ms > 200.0 {
                                Color32::from_rgb(255, 180, 40)
                            } else {
                                Color32::from_rgb(100, 220, 100)
                            };
                            let latency_mode = if pool_telemetry.average_rtt_ms > 200.0 {
                                " (High-Latency WAN Compensation Active)"
                            } else if pool_telemetry.average_rtt_ms > 0.0 {
                                " (LAN / Low-Latency)"
                            } else {
                                " (Idle / No Active Probe)"
                            };
                            ui.label(
                                RichText::new(format!(
                                    "{:.1} ms{}",
                                    pool_telemetry.average_rtt_ms, latency_mode
                                ))
                                .monospace()
                                .color(latency_color),
                            );
                            ui.end_row();
                        });
                })
                .response;

            if std::env::var("MUVRIX_SCROLL_TO_POOL").is_ok() {
                pool_card.scroll_to_me(Some(egui::Align::Center));
            }
        });
    }
}
