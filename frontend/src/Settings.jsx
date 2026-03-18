import React, { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useAuth } from "./AuthContext.jsx";
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
} from "./api.js";

export default function Settings() {
  const { t } = useTranslation();
  const { user } = useAuth();
  const navigate = useNavigate();
  const loadFailedMessageRef = React.useRef(t("settings.loadFailed"));
  const [maxSizeMb, setMaxSizeMb] = useState("");
  const [originalValue, setOriginalValue] = useState(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  const [workspaces, setWorkspaces] = useState([]);
  const [archivedWorkspaces, setArchivedWorkspaces] = useState([]);
  const [workspacesLoading, setWorkspacesLoading] = useState(true);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [newWorkspaceName, setNewWorkspaceName] = useState("");
  const [createError, setCreateError] = useState("");
  const [isCreating, setIsCreating] = useState(false);

  const [showMembersModal, setShowMembersModal] = useState(false);
  const [selectedWorkspace, setSelectedWorkspace] = useState(null);
  const [members, setMembers] = useState([]);
  const [membersLoading, setMembersLoading] = useState(false);
  const [inviteUsername, setInviteUsername] = useState("");
  const [inviteError, setInviteError] = useState("");
  const [isInviting, setIsInviting] = useState(false);
  const inviteFeatureAvailable = true;

  async function refreshWorkspaces() {
    const [ws, archived] = await Promise.all([
      listWorkspaces(),
      listArchivedWorkspaces(),
    ]);
    setWorkspaces(ws);
    setArchivedWorkspaces(archived);
  }

  useEffect(() => {
    loadFailedMessageRef.current = t("settings.loadFailed");
  }, [t]);

  useEffect(() => {
    if (!user) return;
    if (user.role !== "admin") {
      setIsLoading(false);
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
          setError(err.message || loadFailedMessageRef.current);
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
  }, [user]);

  useEffect(() => {
    if (!user) return;

    let cancelled = false;
    async function loadWorkspaces() {
      try {
        const [ws, archived] = await Promise.all([
          listWorkspaces(),
          listArchivedWorkspaces(),
        ]);
        if (!cancelled) {
          setWorkspaces(ws);
          setArchivedWorkspaces(archived);
        }
      } catch (err) {
        console.error("Failed to load workspaces:", err);
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
    setError("");
    setSuccess("");

    const value = parseInt(maxSizeMb, 10);
    if (isNaN(value) || value < 1) {
      setError(t("settings.invalidValue"));
      return;
    }
    if (value > 102400) {
      setError(t("settings.maxValueExceeded"));
      return;
    }

    setIsSaving(true);
    try {
      const data = await updateSettings(value);
      setMaxSizeMb(String(data.maxSizeMb));
      setOriginalValue(data.maxSizeMb);
      setSuccess(t("settings.saved"));
    } catch (err) {
      setError(err.message || t("settings.saveFailed"));
    } finally {
      setIsSaving(false);
    }
  }

  function handleReset() {
    setMaxSizeMb(String(originalValue));
    setError("");
    setSuccess("");
  }

  async function handleCreateWorkspace(e) {
    e.preventDefault();
    setCreateError("");

    const name = newWorkspaceName.trim();
    if (name.length < 3 || name.length > 50) {
      setCreateError(t("workspace.nameLengthError"));
      return;
    }

    setIsCreating(true);
    try {
      await createWorkspace(name);
      await refreshWorkspaces();
      setShowCreateModal(false);
      setNewWorkspaceName("");
    } catch (err) {
      setCreateError(err.message || t("workspace.createFailed"));
    } finally {
      setIsCreating(false);
    }
  }

  async function handleDeleteWorkspace(workspaceId) {
    if (!confirm(t("workspace.deleteConfirm"))) return;

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
      alert(err.message || t("workspace.deleteFailed"));
    }
  }

  async function handleRestoreWorkspace(workspaceId) {
    try {
      await restoreWorkspace(workspaceId);
      await refreshWorkspaces();
    } catch (err) {
      const message = err.message || t("workspace.restoreFailed");
      const isNameConflict = message.includes(t("workspace.nameInUse"));
      if (!isNameConflict) {
        alert(message);
        return;
      }

      const newNameInput = prompt(t("workspace.nameInUsePrompt"));
      if (!newNameInput) {
        return;
      }
      const newName = newNameInput.trim();
      if (newName.length < 3 || newName.length > 50) {
        alert(t("workspace.nameLengthError"));
        return;
      }

      try {
        await restoreWorkspace(workspaceId, newName);
        await refreshWorkspaces();
      } catch (retryErr) {
        alert(retryErr.message || t("workspace.restoreFailed"));
      }
    }
  }

  async function handleOpenMembers(workspace) {
    setSelectedWorkspace(workspace);
    setShowMembersModal(true);
    setMembersLoading(true);
    setInviteUsername("");
    setInviteError("");

    try {
      const data = await listWorkspaceMembers(workspace.id);
      setMembers(data);
    } catch (err) {
      console.error("Failed to load members:", err);
    } finally {
      setMembersLoading(false);
    }
  }

  async function handleInviteMember(e) {
    e.preventDefault();
    setInviteError("");

    const username = inviteUsername.trim();
    if (!username) {
      setInviteError(t("workspace.inviteEmptyError"));
      return;
    }

    setIsInviting(true);
    try {
      const newMember = await inviteWorkspaceMember(
        selectedWorkspace.id,
        username,
      );
      setMembers([...members, newMember]);
      setInviteUsername("");
    } catch (err) {
      setInviteError(err.message || t("workspace.inviteFailed"));
    } finally {
      setIsInviting(false);
    }
  }

  async function handleRemoveMember(userId) {
    if (!confirm(t("workspace.removeConfirm"))) return;

    try {
      await removeWorkspaceMember(selectedWorkspace.id, userId);
      setMembers(members.filter((m) => m.userId !== userId));
    } catch (err) {
      alert(err.message || t("workspace.removeFailed"));
    }
  }

  async function handleLeaveWorkspace(workspaceId) {
    if (!confirm(t("workspace.leaveConfirm"))) return;

    try {
      await leaveWorkspace(workspaceId);
      setWorkspaces(workspaces.filter((w) => w.id !== workspaceId));
    } catch (err) {
      alert(err.message || t("workspace.leaveFailed"));
    }
  }

  const hasChanges = parseInt(maxSizeMb, 10) !== originalValue;
  const isValid =
    !isNaN(parseInt(maxSizeMb, 10)) &&
    parseInt(maxSizeMb, 10) >= 1 &&
    parseInt(maxSizeMb, 10) <= 102400;

  if (!user) {
    return null;
  }

  return (
    <div className="page">
      <header className="header">
        <div>
          <h1>{t("settings.title")}</h1>
          <p className="subtitle">
            {user.role === "admin"
              ? t("settings.subtitleWithWorkspace")
              : t("workspace.management")}
          </p>
        </div>
        <button
          type="button"
          className="btn-secondary"
          onClick={() => navigate("/")}
        >
          {t("common.back")}
        </button>
      </header>

      <section className="panel" style={{ marginTop: "28px" }}>
        <div
          className="panel-header"
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <h2>{t("workspace.management")}</h2>
          <button
            type="button"
            className="btn-primary"
            onClick={() => setShowCreateModal(true)}
          >
            {t("workspace.createNew")}
          </button>
        </div>
        <div className="panel-body" style={{ flexDirection: "column" }}>
          {workspacesLoading ? (
            <div className="empty">{t("common.loading")}</div>
          ) : workspaces.length === 0 ? (
            <div className="empty">{t("workspace.noWorkspaces")}</div>
          ) : (
            workspaces.map((ws) => (
              <div
                key={ws.id}
                style={{
                  padding: "16px",
                  borderBottom: "1px solid #eee",
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                }}
              >
                <div>
                  <div style={{ fontWeight: 500 }}>
                    {ws.name}
                    {ws.isPersonal && (
                      <span
                        style={{
                          marginLeft: "8px",
                          padding: "2px 8px",
                          background: "#e8f4f8",
                          borderRadius: "4px",
                          fontSize: "12px",
                          color: "#0066cc",
                        }}
                      >
                        {t("workspace.personalBadge")}
                      </span>
                    )}
                  </div>
                  <div
                    style={{
                      fontSize: "12px",
                      color: "#666",
                      marginTop: "4px",
                    }}
                  >
                    {t("workspace.memberCount", { count: ws.memberCount })}
                  </div>
                </div>
                <div style={{ display: "flex", gap: "8px" }}>
                  <button
                    type="button"
                    className="btn-secondary"
                    onClick={() => handleOpenMembers(ws)}
                  >
                    {t("workspace.manageMembers")}
                  </button>
                  {!ws.isPersonal && ws.ownerId === user.id && (
                    <button
                      type="button"
                      className="btn-secondary"
                      style={{ color: "#dc3545" }}
                      onClick={() => handleDeleteWorkspace(ws.id)}
                    >
                      {t("common.delete")}
                    </button>
                  )}
                  {!ws.isPersonal && ws.ownerId !== user.id && (
                    <button
                      type="button"
                      className="btn-secondary"
                      style={{ color: "#dc3545" }}
                      onClick={() => handleLeaveWorkspace(ws.id)}
                    >
                      {t("workspace.leave")}
                    </button>
                  )}
                </div>
              </div>
            ))
          )}
        </div>
      </section>

      {archivedWorkspaces.length > 0 && (
        <section className="panel" style={{ marginTop: "20px" }}>
          <div className="panel-header">
            <h2>{t("workspace.archived")}</h2>
          </div>
          <div className="panel-body" style={{ flexDirection: "column" }}>
            {archivedWorkspaces.map((ws) => (
              <div
                key={ws.id}
                style={{
                  padding: "16px",
                  borderBottom: "1px solid #eee",
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                }}
              >
                <div>
                  <div style={{ fontWeight: 500 }}>{ws.name}</div>
                  <div
                    style={{
                      fontSize: "12px",
                      color: "#666",
                      marginTop: "4px",
                    }}
                  >
                    {t("workspace.deletedAt")}{" "}
                    {ws.deletedAt
                      ? new Date(ws.deletedAt).toLocaleString()
                      : t("workspace.unknown")}
                  </div>
                </div>
                {ws.ownerId === user.id && (
                  <button
                    type="button"
                    className="btn-secondary"
                    onClick={() => handleRestoreWorkspace(ws.id)}
                  >
                    {t("workspace.restore")}
                  </button>
                )}
              </div>
            ))}
          </div>
        </section>
      )}

      {user.role === "admin" && (
        <section className="panel" style={{ marginTop: "28px" }}>
          <div className="panel-header">
            <h2>{t("settings.uploadSettings")}</h2>
          </div>
          <div className="panel-body" style={{ flexDirection: "column" }}>
            {isLoading ? (
              <div className="empty">{t("common.loading")}</div>
            ) : (
              <form
                onSubmit={handleSubmit}
                style={{
                  padding: "24px",
                  display: "flex",
                  flexDirection: "column",
                  gap: "20px",
                }}
              >
                {error && <div className="alert">{error}</div>}
                {success && (
                  <div
                    style={{
                      padding: "12px 16px",
                      borderRadius: "10px",
                      background: "#f0fff0",
                      color: "#1a7a1a",
                      border: "1px solid #caf0ca",
                    }}
                  >
                    {success}
                  </div>
                )}

                <div className="detail-group">
                  <div className="detail-label">
                    {t("settings.maxUploadSize")}
                  </div>
                  <div className="detail-value">
                    <input
                      type="number"
                      step="1"
                      min="1"
                      value={maxSizeMb}
                      onChange={(e) => setMaxSizeMb(e.target.value)}
                      className="form-input"
                      style={{ width: "200px" }}
                      disabled={isSaving}
                    />
                    <small
                      className="form-hint"
                      style={{ display: "block", marginTop: "4px" }}
                    >
                      {t("settings.maxSizeHint")}
                    </small>
                  </div>
                </div>

                <div style={{ display: "flex", gap: "8px" }}>
                  <button
                    type="submit"
                    className="btn-primary"
                    disabled={isSaving || !hasChanges || !isValid}
                  >
                    {isSaving ? t("common.saving") : t("common.save")}
                  </button>
                  <button
                    type="button"
                    className="btn-secondary"
                    onClick={handleReset}
                    disabled={isSaving || !hasChanges}
                  >
                    {t("common.reset")}
                  </button>
                </div>
              </form>
            )}
          </div>
        </section>
      )}

      {showCreateModal && (
        <div
          aria-hidden="true"
          style={{
            position: "fixed",
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1000,
          }}
        >
          <button
            type="button"
            aria-label={t("workspace.closeCreateModal")}
            onClick={() => setShowCreateModal(false)}
            style={{
              position: "absolute",
              inset: 0,
              background: "rgba(0,0,0,0.5)",
              border: "none",
              padding: 0,
            }}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="create-workspace-title"
            style={{
              position: "relative",
              background: "white",
              borderRadius: "12px",
              padding: "24px",
              width: "400px",
              maxWidth: "90vw",
            }}
          >
            <h3 id="create-workspace-title" style={{ marginBottom: "16px" }}>
              {t("workspace.createTitle")}
            </h3>
            <form onSubmit={handleCreateWorkspace}>
              <div style={{ marginBottom: "16px" }}>
                <label
                  htmlFor="new-workspace-name"
                  style={{
                    display: "block",
                    marginBottom: "8px",
                    fontWeight: 500,
                  }}
                >
                  {t("workspace.nameLabel")}
                </label>
                <input
                  id="new-workspace-name"
                  type="text"
                  value={newWorkspaceName}
                  onChange={(e) => setNewWorkspaceName(e.target.value)}
                  className="form-input"
                  style={{ width: "100%" }}
                  placeholder={t("workspace.namePlaceholder")}
                />
                {createError && (
                  <div
                    style={{
                      color: "#dc3545",
                      fontSize: "14px",
                      marginTop: "8px",
                    }}
                  >
                    {createError}
                  </div>
                )}
              </div>
              <div
                style={{
                  display: "flex",
                  gap: "8px",
                  justifyContent: "flex-end",
                }}
              >
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={() => setShowCreateModal(false)}
                >
                  {t("common.cancel")}
                </button>
                <button
                  type="submit"
                  className="btn-primary"
                  disabled={isCreating}
                >
                  {isCreating ? t("workspace.creating") : t("workspace.create")}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {showMembersModal && selectedWorkspace && (
        <div
          aria-hidden="true"
          style={{
            position: "fixed",
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            zIndex: 1000,
          }}
        >
          <button
            type="button"
            aria-label={t("workspace.closeMembersModal")}
            onClick={() => setShowMembersModal(false)}
            style={{
              position: "absolute",
              inset: 0,
              background: "rgba(0,0,0,0.5)",
              border: "none",
              padding: 0,
            }}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="workspace-members-title"
            style={{
              position: "relative",
              background: "white",
              borderRadius: "12px",
              padding: "24px",
              width: "500px",
              maxWidth: "90vw",
              maxHeight: "80vh",
              overflow: "auto",
            }}
          >
            <h3 id="workspace-members-title" style={{ marginBottom: "16px" }}>
              {t("workspace.membersTitle", { name: selectedWorkspace.name })}
            </h3>

            {inviteFeatureAvailable ? (
              <form
                onSubmit={handleInviteMember}
                style={{ marginBottom: "20px" }}
              >
                <div style={{ display: "flex", gap: "8px" }}>
                  <input
                    type="text"
                    value={inviteUsername}
                    onChange={(e) => setInviteUsername(e.target.value)}
                    className="form-input"
                    style={{ flex: 1 }}
                    placeholder={t("workspace.invitePlaceholder")}
                  />
                  <button
                    type="submit"
                    className="btn-primary"
                    disabled={isInviting}
                  >
                    {isInviting
                      ? t("workspace.inviting")
                      : t("workspace.invite")}
                  </button>
                </div>
                {inviteError && (
                  <div
                    style={{
                      color: "#dc3545",
                      fontSize: "14px",
                      marginTop: "8px",
                    }}
                  >
                    {inviteError}
                  </div>
                )}
              </form>
            ) : (
              <div
                style={{
                  marginBottom: "20px",
                  padding: "10px 12px",
                  background: "#f8f9fa",
                  border: "1px solid #e5e7eb",
                  borderRadius: "8px",
                  fontSize: "14px",
                  color: "#495057",
                }}
              >
                {t("workspace.inviteUnavailable")}
              </div>
            )}

            <div style={{ marginBottom: "8px", fontWeight: 500 }}>
              {t("workspace.currentMembers")} ({members.length})
            </div>
            {membersLoading ? (
              <div className="empty">{t("common.loading")}</div>
            ) : (
              <div style={{ maxHeight: "300px", overflow: "auto" }}>
                {members.map((m) => (
                  <div
                    key={m.userId}
                    style={{
                      padding: "12px",
                      borderBottom: "1px solid #eee",
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "center",
                    }}
                  >
                    <div>
                      <span style={{ fontWeight: 500 }}>{m.username}</span>
                      {m.isOwner && (
                        <span
                          style={{
                            marginLeft: "8px",
                            fontSize: "12px",
                            color: "#666",
                          }}
                        >
                          {t("workspace.owner")}
                        </span>
                      )}
                    </div>
                    {!m.isOwner && selectedWorkspace.ownerId === user.id && (
                      <button
                        type="button"
                        className="btn-secondary"
                        style={{
                          fontSize: "12px",
                          padding: "4px 8px",
                          color: "#dc3545",
                        }}
                        onClick={() => handleRemoveMember(m.userId)}
                      >
                        {t("workspace.remove")}
                      </button>
                    )}
                  </div>
                ))}
              </div>
            )}

            <div
              style={{
                marginTop: "20px",
                display: "flex",
                justifyContent: "flex-end",
              }}
            >
              <button
                type="button"
                className="btn-secondary"
                onClick={() => setShowMembersModal(false)}
              >
                {t("common.close")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
