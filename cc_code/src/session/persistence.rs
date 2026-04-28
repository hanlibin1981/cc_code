//! Session 持久化辅助函数
//! 会话保存/加载逻辑（供 SessionManager 调用）

use crate::session::{Session, SESSION_VERSION};
use std::fs;
use std::io::{self, BufReader, BufWriter};
use std::path::Path;
use tracing::{error, info};

/// 将 session 序列化并写入文件
pub fn write_session_file(session: &Session, dir: &Path) -> io::Result<()> {
    #[derive(serde::Serialize)]
    struct SessionMeta<'a> {
        version: u32,
        session: &'a Session,
    }

    fs::create_dir_all(dir)?;
    let file_path = dir.join(format!("{}.json", session.id));
    let file = fs::File::create(&file_path)?;
    let writer = BufWriter::new(file);

    let meta = SessionMeta {
        version: SESSION_VERSION,
        session,
    };

    serde_json::to_writer(writer, &meta)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(())
}

/// 从文件反序列化 session
pub fn read_session_file(path: &Path) -> io::Result<Session> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    #[derive(serde::Deserialize)]
    struct SessionMeta {
        version: u32,
        session: Session,
    }

    let meta: SessionMeta = serde_json::from_reader(reader)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    if meta.version != SESSION_VERSION {
        error!("会话版本不匹配: {} vs {}", meta.version, SESSION_VERSION);
    }

    Ok(meta.session)
}

/// 从目录加载所有会话文件
pub fn load_sessions_from_dir(dir: &Path) -> io::Result<Vec<Session>> {
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut sessions = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match read_session_file(&path) {
            Ok(session) => {
                info!("加载会话 {:?}", path);
                sessions.push(session);
            }
            Err(e) => {
                error!("加载会话失败 {:?}: {}", path, e);
            }
        }
    }
    Ok(sessions)
}

/// 获取默认数据目录
pub fn default_data_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("cc_code")
        .join("sessions")
}
