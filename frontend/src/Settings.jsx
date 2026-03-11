import React, { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from './AuthContext.jsx';
import {
  getSettings,
  updateSettings,
  listWorkspaces,
  createWorkspace,
  updateWorkspace,
  deleteWorkspace,
  restoreWorkspace,
  listArchivedWorkspaces,
  listWorkspaceMembers,
  inviteWorkspaceMember,
  removeWorkspaceMember,
  leaveWorkspace,
} from './api.js';

export default function Settings() {
  const { user } = useAuth();
  const navigate = useNavigate();
  const [maxSizeMb, setMaxSizeMb] = useState('');
  const [originalValue, setOriginalValue] = useState(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');

  const [workspaces, setWorkspaces] = useState([]);
  const [archivedWorkspaces, setArchivedWorkspaces] = useState([]);
  const [workspacesLoading, setWorkspacesLoading] = useState(true);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newWorkspaceName, setNewWorkspaceName] = useState('');
  const [createError, setCreateError] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  const [showMembersModal, setShowMembersModal] = useState(false);
  const [selectedWorkspace, setSelectedWorkspace] = useState(null);
  const [members, setMembers] = useState([]);
  const [membersLoading, setMembersLoading] = useState(false);
  const [inviteUsername, setInviteUsername] = useState('');
  const [inviteError, setInviteError] = useState('');
  const [isInviting, setIsInviting] = useState(false);

  useEffect(() => {
    if (!user) return;
    if (user.role !== 'admin') {
      navigate('/');
      return;
    }

    let cancelled = false;
    async function load() {
      try {
        const data = await getSettings();
        if (!cancelled) {
          setMaxSizeMb(String(data.maxSizeMb));
          setOriginalValue(data.maxSizeMb);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err.message || '加载设置失败');
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [user, navigate]);

  useEffect(() => {
    if (!user) return;

    let cancelled = false;
    async function loadWorkspaces() {
      try {
        const [ws, archived] = await Promise.all([listWorkspaces(), listArchivedWorkspaces()]);
        if (!cancelled) {
          setWorkspaces(ws);
          setArchivedWorkspaces(archived);
        }
      } catch (err) {
        console.error('Failed to load workspaces:', err);
      } finally {
        if (!cancelled) {
          setWorkspacesLoading(false);
        }
      }
    }
    loadWorkspaces();
    return () => {
      cancelled = true;
    };
  }, [user]);

  async function handleSubmit(e) {
    e.preventDefault();
    setError('');
    setSuccess('');

    const value = parseInt(maxSizeMb, 10);
    if (isNaN(value) || value < 1) {
      setError('请输入有效的数值（最小 1 MB）');
      return;
    }
    if (value > 102400) {
      setError('最大值为 100 GB（102400 MB）');
      return;
    }

    setIsSaving(true);
    try {
      const data = await updateSettings(value);
      setMaxSizeMb(String(data.maxSizeMb));
      setOriginalValue(data.maxSizeMb);
      setSuccess('设置已保存');
    } catch (err) {
      setError(err.message || '保存失败');
    } finally {
      setIsSaving(false);
    }
  }

  function handleReset() {
    setMaxSizeMb(String(originalValue));
    setError('');
    setSuccess('');
  }

  async function handleCreateWorkspace(e) {
    e.preventDefault();
    setCreateError('');

    const name = newWorkspaceName.trim();
    if (name.length < 3 || name.length > 50) {
      setCreateError('名称长度需要在 3-50 个字符之间');
      return;
    }

    setIsCreating(true);
    try {
      const ws = await createWorkspace(name);
      setWorkspaces([...workspaces, ws]);
      setShowCreateModal(false);
      setNewWorkspaceName('');
    } catch (err) {
      setCreateError(err.message || '创建失败');
    } finally {
      setIsCreating(false);
    }
  }

  async function handleDeleteWorkspace(workspaceId) {
    if (!confirm('确定要删除此工作空间吗？删除后可以恢复。')) return;

    try {
      await deleteWorkspace(workspaceId);
      const ws = workspaces.find((w) => w.id === workspaceId);
      setWorkspaces(workspaces.filter((w) => w.id !== workspaceId));
      if (ws) {
        setArchivedWorkspaces([
          { ...ws, deletedAt: new Date().toISOString() },
          ...archivedWorkspaces,
        ]);
      }
    } catch (err) {
      alert(err.message || '删除失败');
    }
  }

  async function handleRestoreWorkspace(workspaceId) {
    try {
      const ws = await restoreWorkspace(workspaceId);
      setArchivedWorkspaces(archivedWorkspaces.filter((w) => w.id !== workspaceId));
      setWorkspaces([...workspaces, ws]);
    } catch (err) {
      alert(err.message || '恢复失败');
    }
  }

  async function handleOpenMembers(workspace) {
    setSelectedWorkspace(workspace);
    setShowMembersModal(true);
    setMembersLoading(true);
    setInviteUsername('');
    setInviteError('');

    try {
      const data = await listWorkspaceMembers(workspace.id);
      setMembers(data);
    } catch (err) {
      console.error('Failed to load members:', err);
    } finally {
      setMembersLoading(false);
    }
  }

  async function handleInviteMember(e) {
    e.preventDefault();
    setInviteError('');

    const username = inviteUsername.trim();
    if (!username) {
      setInviteError('请输入用户名');
      return;
    }

    setIsInviting(true);
    try {
      const newMember = await inviteWorkspaceMember(selectedWorkspace.id, username);
      setMembers([...members, newMember]);
      setInviteUsername('');
    } catch (err) {
      setInviteError(err.message || '邀请失败');
    } finally {
      setIsInviting(false);
    }
  }

  async function handleRemoveMember(userId) {
    if (!confirm('确定要移除此成员吗？')) return;

    try {
      await removeWorkspaceMember(selectedWorkspace.id, userId);
      setMembers(members.filter((m) => m.userId !== userId));
    } catch (err) {
      alert(err.message || '移除失败');
    }
  }

  async function handleLeaveWorkspace(workspaceId) {
    if (!confirm('确定要离开此工作空间吗？')) return;

    try {
      await leaveWorkspace(workspaceId);
      setWorkspaces(workspaces.filter((w) => w.id !== workspaceId));
    } catch (err) {
      alert(err.message || '离开失败');
    }
  }

  const hasChanges = parseInt(maxSizeMb, 10) !== originalValue;
  const isValid =
    !isNaN(parseInt(maxSizeMb, 10)) &&
    parseInt(maxSizeMb, 10) >= 1 &&
    parseInt(maxSizeMb, 10) <= 102400;

  if (!user || user.role !== 'admin') {
    return null;
  }

  return (
    <div className="page">
      <header className="header">
        <div>
          <h1>设置</h1>
          <p className="subtitle">系统配置</p>
        </div>
        <button type="button" className="btn-secondary" onClick={() => navigate('/')}>
          返回
        </button>
      </header>

      <section className="panel" style={{ marginTop: '28px' }}>
        <div
          className="panel-header"
          style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}
        >
          <h2>工作空间管理</h2>
          <button type="button" className="btn-primary" onClick={() => setShowCreateModal(true)}>
            + 新建
          </button>
        </div>
        <div className="panel-body" style={{ flexDirection: 'column' }}>
          {workspacesLoading ? (
            <div className="empty">加载中...</div>
          ) : workspaces.length === 0 ? (
            <div className="empty">暂无工作空间</div>
          ) : (
            workspaces.map((ws) => (
              <div
                key={ws.id}
                style={{
                  padding: '16px',
                  borderBottom: '1px solid #eee',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                }}
              >
                <div>
                  <div style={{ fontWeight: 500 }}>
                    {ws.name}
                    {ws.isPersonal && (
                      <span
                        style={{
                          marginLeft: '8px',
                          padding: '2px 8px',
                          background: '#e8f4f8',
                          borderRadius: '4px',
                          fontSize: '12px',
                          color: '#0066cc',
                        }}
                      >
                        个人
                      </span>
                    )}
                  </div>
                  <div style={{ fontSize: '12px', color: '#666', marginTop: '4px' }}>
                    {ws.memberCount} 个成员
                  </div>
                </div>
                <div style={{ display: 'flex', gap: '8px' }}>
                  <button
                    type="button"
                    className="btn-secondary"
                    onClick={() => handleOpenMembers(ws)}
                  >
                    管理成员
                  </button>
                  {!ws.isPersonal && ws.ownerId === user.id && (
                    <button
                      type="button"
                      className="btn-secondary"
                      style={{ color: '#dc3545' }}
                      onClick={() => handleDeleteWorkspace(ws.id)}
                    >
                      删除
                    </button>
                  )}
                  {!ws.isPersonal && ws.ownerId !== user.id && (
                    <button
                      type="button"
                      className="btn-secondary"
                      style={{ color: '#dc3545' }}
                      onClick={() => handleLeaveWorkspace(ws.id)}
                    >
                      离开
                    </button>
                  )}
                </div>
              </div>
            ))
          )}
        </div>
      </section>

      {archivedWorkspaces.length > 0 && (
        <section className="panel" style={{ marginTop: '20px' }}>
          <div className="panel-header">
            <h2>已归档的工作空间</h2>
          </div>
          <div className="panel-body" style={{ flexDirection: 'column' }}>
            {archivedWorkspaces.map((ws) => (
              <div
                key={ws.id}
                style={{
                  padding: '16px',
                  borderBottom: '1px solid #eee',
                  display: 'flex',
                  justifyContent: 'space-between',
                  alignItems: 'center',
                }}
              >
                <div>
                  <div style={{ fontWeight: 500 }}>{ws.name}</div>
                  <div style={{ fontSize: '12px', color: '#666', marginTop: '4px' }}>
                    删除于 {ws.deletedAt ? new Date(ws.deletedAt).toLocaleString() : '未知'}
                  </div>
                </div>
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={() => handleRestoreWorkspace(ws.id)}
                >
                  恢复
                </button>
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="panel" style={{ marginTop: '28px' }}>
        <div className="panel-header">
          <h2>上传设置</h2>
        </div>
        <div className="panel-body" style={{ flexDirection: 'column' }}>
          {isLoading ? (
            <div className="empty">加载中...</div>
          ) : (
            <form
              onSubmit={handleSubmit}
              style={{ padding: '24px', display: 'flex', flexDirection: 'column', gap: '20px' }}
            >
              {error && <div className="alert">{error}</div>}
              {success && (
                <div
                  style={{
                    padding: '12px 16px',
                    borderRadius: '10px',
                    background: '#f0fff0',
                    color: '#1a7a1a',
                    border: '1px solid #caf0ca',
                  }}
                >
                  {success}
                </div>
              )}

              <div className="detail-group">
                <div className="detail-label">最大上传大小 (MB)</div>
                <div className="detail-value">
                  <input
                    type="number"
                    step="1"
                    min="1"
                    value={maxSizeMb}
                    onChange={(e) => setMaxSizeMb(e.target.value)}
                    className="form-input"
                    style={{ width: '200px' }}
                    disabled={isSaving}
                  />
                  <small className="form-hint" style={{ display: 'block', marginTop: '4px' }}>
                    范围：1 MB - 100 GB
                  </small>
                </div>
              </div>

              <div style={{ display: 'flex', gap: '8px' }}>
                <button
                  type="submit"
                  className="btn-primary"
                  disabled={isSaving || !hasChanges || !isValid}
                >
                  {isSaving ? '保存中...' : '保存'}
                </button>
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={handleReset}
                  disabled={isSaving || !hasChanges}
                >
                  重置
                </button>
              </div>
            </form>
          )}
        </div>
      </section>

      {showCreateModal && (
        <div
          style={{
            position: 'fixed',
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            background: 'rgba(0,0,0,0.5)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            zIndex: 1000,
          }}
          onClick={() => setShowCreateModal(false)}
        >
          <div
            style={{
              background: 'white',
              borderRadius: '12px',
              padding: '24px',
              width: '400px',
              maxWidth: '90vw',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <h3 style={{ marginBottom: '16px' }}>创建工作空间</h3>
            <form onSubmit={handleCreateWorkspace}>
              <div style={{ marginBottom: '16px' }}>
                <label style={{ display: 'block', marginBottom: '8px', fontWeight: 500 }}>
                  工作空间名称
                </label>
                <input
                  type="text"
                  value={newWorkspaceName}
                  onChange={(e) => setNewWorkspaceName(e.target.value)}
                  className="form-input"
                  style={{ width: '100%' }}
                  placeholder="3-50 个字符"
                  autoFocus
                />
                {createError && (
                  <div style={{ color: '#dc3545', fontSize: '14px', marginTop: '8px' }}>
                    {createError}
                  </div>
                )}
              </div>
              <div style={{ display: 'flex', gap: '8px', justifyContent: 'flex-end' }}>
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={() => setShowCreateModal(false)}
                >
                  取消
                </button>
                <button type="submit" className="btn-primary" disabled={isCreating}>
                  {isCreating ? '创建中...' : '创建'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {showMembersModal && selectedWorkspace && (
        <div
          style={{
            position: 'fixed',
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            background: 'rgba(0,0,0,0.5)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            zIndex: 1000,
          }}
          onClick={() => setShowMembersModal(false)}
        >
          <div
            style={{
              background: 'white',
              borderRadius: '12px',
              padding: '24px',
              width: '500px',
              maxWidth: '90vw',
              maxHeight: '80vh',
              overflow: 'auto',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <h3 style={{ marginBottom: '16px' }}>成员管理 - {selectedWorkspace.name}</h3>

            <form onSubmit={handleInviteMember} style={{ marginBottom: '20px' }}>
              <div style={{ display: 'flex', gap: '8px' }}>
                <input
                  type="text"
                  value={inviteUsername}
                  onChange={(e) => setInviteUsername(e.target.value)}
                  className="form-input"
                  style={{ flex: 1 }}
                  placeholder="输入用户名邀请..."
                />
                <button type="submit" className="btn-primary" disabled={isInviting}>
                  {isInviting ? '邀请中...' : '邀请'}
                </button>
              </div>
              {inviteError && (
                <div style={{ color: '#dc3545', fontSize: '14px', marginTop: '8px' }}>
                  {inviteError}
                </div>
              )}
            </form>

            <div style={{ marginBottom: '8px', fontWeight: 500 }}>当前成员 ({members.length})</div>
            {membersLoading ? (
              <div className="empty">加载中...</div>
            ) : (
              <div style={{ maxHeight: '300px', overflow: 'auto' }}>
                {members.map((m) => (
                  <div
                    key={m.userId}
                    style={{
                      padding: '12px',
                      borderBottom: '1px solid #eee',
                      display: 'flex',
                      justifyContent: 'space-between',
                      alignItems: 'center',
                    }}
                  >
                    <div>
                      <span style={{ fontWeight: 500 }}>{m.username}</span>
                      {m.isOwner && (
                        <span style={{ marginLeft: '8px', fontSize: '12px', color: '#666' }}>
                          Owner
                        </span>
                      )}
                    </div>
                    {!m.isOwner && selectedWorkspace.ownerId === user.id && (
                      <button
                        type="button"
                        className="btn-secondary"
                        style={{ fontSize: '12px', padding: '4px 8px', color: '#dc3545' }}
                        onClick={() => handleRemoveMember(m.userId)}
                      >
                        移除
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}

            <div style={{ marginTop: '20px', display: 'flex', justifyContent: 'flex-end' }}>
              <button
                type="button"
                className="btn-secondary"
                onClick={() => setShowMembersModal(false)}
              >
                关闭
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
