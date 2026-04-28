//! Session 持久化模块
//! 将会话状态保存到磁盘，重启后可恢复

use crate::session::{Session, SessionManager};
use std::fs;
use std::io::{self, BufReader, BufWriter};
use std::path::Path;
use tracing::{error, info};

/// Session 文件格式版本
const SESSION_VERSION: u32 = 1;

/// 持久化会话元数据（不含消息体，用于列表展示）
#[derive(serde::Serialize, serde::Deserialize)]
struct SessionMeta {
    version: u32,
    session: Session,
}

impl SessionManager {
    /// 将所有会话持久化到指定目录
    pub fn persist_to_dir(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;

        for session in self.sessions.values() {
            let file_path = dir.join(format!("{}.json", session.id));
            let file = fs::File::create(&file_path)?;
            let writer = BufWriter::new(file);

            let meta = SessionMeta {
                version: SESSION_VERSION,
                session: session.clone(),
            };

            serde_json::to_writer(writer, &meta)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        }

        info!("已保存 {} 个会话到 {:?}", self.sessions.len(), dir);
        Ok(())
    }

    /// 从指定目录加载所有会话
    pub fn load_from_dir(&mut self, dir: &Path) -> io::Result<usize> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut loaded = 0;

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            match Self::load_session_file(&path) {
                Ok(session) => {
                    let id = session.id;
                    if self.sessions.insert(id, session).is_none() {
                        loaded += 1;
                    }
                }
                Err(e) => {
                    error!("加载会话失败 {:?}: {}", path, e);
                }
            }
        }

        info!("从 {:?} 加载了 {} 个会话", dir, loaded);
        Ok(loaded)
    }

    /// 加载单个会话文件
    fn load_session_file(path: &Path) -> io::Result<Session> {
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);

        let meta: SessionMeta = serde_json::from_reader(reader)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        if meta.version != SESSION_VERSION {
            error!("会话版本不匹配: {} vs {}", meta.version, SESSION_VERSION);
        }

        Ok(meta.session)
    }

    /// 保存指定会话到文件
    pub fn save_session(&self, id: &uuid::Uuid, dir: &Path) -> io::Result<()> {
        let session = self.sessions.get(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Session not found"))?;

        fs::create_dir_all(dir)?;
        let file_path = dir.join(format!("{}.json", id));
        let file = fs::File::create(&file_path)?;
        let writer = BufWriter::new(file);

        let meta = SessionMeta {
            version: SESSION_VERSION,
            session: session.clone(),
        };

        serde_json::to_writer(writer, &meta)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        Ok(())
    }

    /// 获取数据目录（默认 ~/.local/share/cc_code）
    pub fn default_data_dir() -> std::path::PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("cc_code")
            .join("sessions")
    }
}
