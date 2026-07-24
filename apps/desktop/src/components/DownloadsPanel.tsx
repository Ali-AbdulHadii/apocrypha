/**
 * Downloads settings: where mods come from, the Nexus account, and whether this
 * app handles `nxm://` links.
 *
 * The honest constraint is stated in the UI rather than hidden: a free Nexus
 * account cannot have a manager fetch files on its behalf. Nexus refuses those
 * requests unless the link carries a token that only the website issues when
 * the user presses "Mod Manager Download". So for a free account the browser
 * step is part of the design, not a failure.
 */

import { useEffect, useState } from "react";
import { api, type NexusStatusView } from "../lib/api";
import { Icon } from "./icons";
import { Chip, Segmented } from "./ui";

export function DownloadsPanel({
  onError,
  onInfo,
}: {
  onError: (e: unknown) => void;
  onInfo: (msg: string, kind?: "ok" | "bad" | "info") => void;
}) {
  const [status, setStatus] = useState<NexusStatusView | null>(null);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.nexusStatus().then(setStatus).catch(onError);
  }, [onError]);

  if (!status) return null;

  async function run<T>(fn: () => Promise<T>, ok?: string) {
    setBusy(true);
    try {
      const r = await fn();
      if (ok) onInfo(ok, "ok");
      return r;
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  }

  const nexus = status.source === "nexus";

  return (
    <div className="card stack">
      <div className="card-title">Downloads</div>

      <div className="field">
        <label>Get mods from</label>
        <Segmented
          idPrefix="dlsource"
          value={status.source as "nexus" | "apocrypha"}
          onChange={async (v) => {
            const next = await run(() => api.setDownloadSource(v));
            if (next) setStatus(next);
          }}
          options={[
            { value: "nexus", label: "Nexus Mods" },
            { value: "apocrypha", label: "Apocrypha" },
          ]}
        />
        {!nexus && (
          <div className="card-hint">
            The Apocrypha service is not available yet. Downloads still come from
            Nexus Mods until it is.
          </div>
        )}
      </div>

      <div className="divider" />

      <div className="field">
        <label>Handle download links from the website</label>
        <div className="row">
          {status.handlerIsDefault ? (
            <Chip kind="ok">
              <span className="dot" /> Apocrypha handles them
            </Chip>
          ) : status.currentHandler ? (
            <Chip kind="warn">Handled by {status.currentHandler}</Chip>
          ) : (
            <Chip kind="warn">Not set up</Chip>
          )}
          <div style={{ marginLeft: "auto" }} className="row">
            {status.handlerIsDefault ? (
              <button
                className="btn sm"
                disabled={busy}
                onClick={async () => {
                  const next = await run(
                    () => api.unregisterNxmHandler(),
                    "Apocrypha no longer handles those links",
                  );
                  if (next) setStatus(next);
                }}
              >
                Turn off
              </button>
            ) : (
              <button
                className="btn sm primary"
                disabled={busy}
                onClick={async () => {
                  const next = await run(
                    () => api.registerNxmHandler(),
                    "Apocrypha will now open Nexus Mods download links",
                  );
                  if (next) setStatus(next);
                }}
              >
                Set up
              </button>
            )}
          </div>
        </div>
        <div className="card-hint">
          Lets the Mod Manager Download button on Nexus Mods send the file
          straight here instead of you saving it by hand.
        </div>
      </div>

      <div className="divider" />

      <div className="field">
        <label>Nexus Mods account</label>
        {status.hasApiKey ? (
          <div className="row">
            <Chip kind="ok">
              <span className="dot" /> {status.userName ?? "Signed in"}
            </Chip>
            {status.isPremium ? (
              <Chip kind="accent">Premium</Chip>
            ) : (
              <Chip>Free account</Chip>
            )}
            <div style={{ marginLeft: "auto" }}>
              <button
                className="btn sm"
                disabled={busy}
                onClick={async () => {
                  const next = await run(
                    () => api.setNexusApiKey(""),
                    "Nexus Mods key removed",
                  );
                  if (next) setStatus(next);
                  setKey("");
                }}
              >
                Remove
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="row">
              <button
                className="btn primary"
                disabled={busy || !status.canSignIn}
                onClick={async () => {
                  onInfo("Approve the sign-in in your browser", "info");
                  const next = await run(
                    () => api.nexusSignIn(),
                    "Signed in to Nexus Mods",
                  );
                  if (next) setStatus(next);
                }}
              >
                Sign in with Nexus Mods
              </button>
              <span className="card-hint">
                Opens your browser, no key to copy.
              </span>
            </div>

            {!status.canSignIn && (
              <div className="notice">
                <span style={{ flexShrink: 0 }}>
                  <Icon.info />
                </span>
                <div>
                  <div className="notice-title">Browser sign-in is not available yet</div>
                  <div className="notice-body">
                    Nexus Mods only issues the application id this needs to
                    developers who ask for one, and Apocrypha does not have one
                    yet. Paste a personal API key below in the meantime. If you
                    have been given an application id, enter it under Advanced.
                  </div>
                </div>
              </div>
            )}

            <div className="divider" />

            <div className="field">
              <label>Or paste a personal API key</label>
              <div className="row">
                <input
                  className="input"
                  style={{ flex: 1 }}
                  type="password"
                  placeholder="Personal API key"
                  value={key}
                  onChange={(e) => setKey(e.target.value)}
                />
                <button
                  className="btn"
                  disabled={busy || !key.trim()}
                  onClick={async () => {
                    const next = await run(
                      () => api.setNexusApiKey(key.trim()),
                      "Nexus Mods account connected",
                    );
                    if (next) {
                      setStatus(next);
                      setKey("");
                    }
                  }}
                >
                  Connect
                </button>
              </div>
              <div className="card-hint">
                Kept on this computer and only ever sent to Nexus Mods.
                <button
                  className="btn sm ghost"
                  style={{ marginLeft: "var(--sp-3)" }}
                  onClick={() =>
                    api
                      .openUrl("https://next.nexusmods.com/settings/api-keys")
                      .catch(onError)
                  }
                >
                  Get a key
                </button>
              </div>
            </div>

            <details>
              <summary
                className="card-hint"
                style={{ cursor: "pointer", userSelect: "none" }}
              >
                Advanced
              </summary>
              <div className="field" style={{ marginTop: "var(--sp-3)" }}>
                <label>Nexus Mods application id</label>
                <div className="row">
                  <input
                    className="input"
                    style={{ flex: 1 }}
                    placeholder="Issued by Nexus Mods"
                    defaultValue={status.ssoApplication}
                    onBlur={async (e) => {
                      const v = e.target.value.trim();
                      if (v === status.ssoApplication) return;
                      const next = await run(() => api.setSsoApplication(v));
                      if (next) setStatus(next);
                    }}
                  />
                </div>
                <div className="card-hint">
                  Enables the sign-in button above. Only Nexus Mods staff can
                  issue one.
                </div>
              </div>
            </details>
          </>
        )}
      </div>

      {status.hasApiKey && !status.isPremium && (
        <div className="notice info">
          <span style={{ flexShrink: 0 }}>
            <Icon.info />
          </span>
          <div>
            <div className="notice-title">How downloading works on a free account</div>
            <div className="notice-body">
              Nexus Mods only lets a mod manager download a file after you press
              Mod Manager Download on the website. Apocrypha will open the mod
              page for you, and the moment you press that button the file comes
              straight here. This is a Nexus Mods rule, not a missing feature.
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
