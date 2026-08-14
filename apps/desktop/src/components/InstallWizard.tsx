/**
 * The install wizard: a FOMOD-style conditional installer driven entirely by
 * parsed mod metadata.
 *
 * Rendering rules come from the option's `selectMode` and `radioSet`:
 *   forced     → locked, pre-checked (e.g. "-0- basic files (must install)")
 *   exclusive  → radio, one per `radioSet` (Physics-body and Physics-leg are
 *                separate sets even though they share a group)
 *   stackable  → checkbox addon that layers over the base variant
 *   info       → non-interactive notice (cover art / warnings), never installed
 */

import { AnimatePresence, motion } from "framer-motion";
import { useEffect, useMemo, useState } from "react";
import {
  formatBytes,
  type CarryView,
  type GroupView,
  type ModView,
  type OptionView,
  type PreviewSource,
} from "../lib/api";
import { OptionThumb, PreviewHero, prefetchPreviews } from "./Preview";
import { Chip, Icon } from "./ui";

interface Props {
  mod: ModView;
  busy?: boolean;
  /** Where option preview images are fetched from. */
  previewSource: PreviewSource | null;
  /**
   * Set when this archive updates a mod already installed and the carry left
   * something to ask about. A complete carry never reaches the wizard — the
   * caller installs it directly — so this is always an incomplete one.
   */
  carry?: CarryView | null;
  onCancel: () => void;
  onConfirm: (selection: string[]) => void;
  confirmLabel?: string;
}

/** Options in a group that are not part of any radio set. */
function looseOptions(group: GroupView): OptionView[] {
  return group.options.filter((o) => !o.radioSet);
}

/**
 * Split a group's non-radio options by role. The wizard presents them in the
 * same order the deployment engine layers them: required files first, then the
 * base variant choice, then addons that override on top.
 */
function splitLoose(group: GroupView): {
  notices: OptionView[];
  required: OptionView[];
  addons: OptionView[];
} {
  const loose = looseOptions(group);
  return {
    notices: loose.filter((o) => o.selectMode === "info"),
    required: loose.filter((o) => o.selectMode === "forced"),
    addons: loose.filter((o) => o.selectMode === "stackable"),
  };
}

function setLabel(key: string): string {
  const raw = key.includes(":") ? key.slice(key.indexOf(":") + 1) : key;
  return raw.replace(/[-_]+/g, " ").trim() || "Choice";
}

/**
 * Explain why the wizard opened on an update instead of installing silently.
 *
 * Both halves are worth stating separately because they are different problems:
 * a dropped option is something the person had and no longer can have, and an
 * undecided set is a question this version asks that the last one did not.
 * Returns null when there is nothing to say, so the banner disappears rather
 * than rendering empty.
 */
function carryMessage(carry: CarryView): string | null {
  const parts: string[] = [];
  if (carry.dropped.length > 0) {
    parts.push(
      `${carry.dropped.length === 1 ? "An option you chose is" : `${carry.dropped.length} options you chose are`} not in this version: ${carry.dropped.join(", ")}.`,
    );
  }
  if (carry.undecided.length > 0) {
    parts.push(
      `${carry.undecided.length === 1 ? "This choice needs" : "These choices need"} an answer: ${carry.undecided.map(setLabel).join(", ")}.`,
    );
  }
  if (parts.length === 0) return null;
  const kept =
    carry.carried.length > 0
      ? ` Your other ${carry.carried.length === 1 ? "choice is" : `${carry.carried.length} choices are`} already ticked.`
      : "";
  return `${parts.join(" ")}${kept}`;
}

export function InstallWizard({
  mod,
  busy,
  previewSource,
  carry,
  onCancel,
  onConfirm,
  confirmLabel = "Install",
}: Props) {
  const [selection, setSelection] = useState<Set<string>>(
    () => new Set(mod.selection),
  );
  const [stepIndex, setStepIndex] = useState(0);

  const steps = mod.groups;
  const step = steps[stepIndex];

  // Warm the images for the current and next step. Without this, moving to a
  // step fetches every thumbnail at once and the transition visibly hitches.
  useEffect(() => {
    const ids = [steps[stepIndex], steps[stepIndex + 1]]
      .filter(Boolean)
      .flatMap((g) => g.options)
      .filter((o) => o.hasPreview)
      .map((o) => o.id);
    prefetchPreviews(previewSource, ids);
  }, [steps, stepIndex, previewSource]);

  /** A step is "resolved" once every radio set in it has a choice. */
  const stepResolved = useMemo(() => {
    return steps.map((g) =>
      g.radioSets.every((key) =>
        g.options.some((o) => o.radioSet === key && selection.has(o.id)),
      ),
    );
  }, [steps, selection]);

  const chosenOptions = useMemo(
    () => mod.groups.flatMap((g) => g.options).filter((o) => selection.has(o.id)),
    [mod.groups, selection],
  );

  const totals = useMemo(
    () => ({
      files: chosenOptions.reduce((n, o) => n + o.fileCount, 0),
      bytes: chosenOptions.reduce((n, o) => n + o.sizeBytes, 0),
    }),
    [chosenOptions],
  );

  const unresolved = stepResolved.filter((r) => !r).length;

  function pickRadio(group: GroupView, option: OptionView) {
    setSelection((prev) => {
      const next = new Set(prev);
      // Deselect every sibling in the same radio set, then select this one.
      for (const sibling of group.options) {
        if (sibling.radioSet && sibling.radioSet === option.radioSet) {
          next.delete(sibling.id);
        }
      }
      if (!prev.has(option.id)) next.add(option.id);
      return next;
    });
  }

  function toggleAddon(option: OptionView) {
    setSelection((prev) => {
      const next = new Set(prev);
      if (next.has(option.id)) next.delete(option.id);
      else next.add(option.id);
      return next;
    });
  }

  return (
    <motion.div
      className="overlay"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.18 }}
      onClick={(e) => {
        if (e.target === e.currentTarget && !busy) onCancel();
      }}
    >
      <motion.div
        className="wizard"
        initial={{ opacity: 0, scale: 0.97, y: 12 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.98, y: 8 }}
        transition={{ type: "spring", stiffness: 380, damping: 34 }}
        role="dialog"
        aria-modal="true"
        aria-label={`Install ${mod.name}`}
      >
        <header className="wizard-header">
          <div style={{ minWidth: 0 }}>
            <div className="row" style={{ gap: 8 }}>
              <h2 style={{ fontSize: "var(--text-lg)", fontWeight: 650 }}>
                {mod.name}
              </h2>
              {mod.version && <Chip>{mod.version}</Chip>}
              <Chip kind="accent">{mod.installerModel}</Chip>
            </div>
            <div className="card-hint">
              {mod.author ? `by ${mod.author} · ` : ""}
              {mod.groups.reduce((n, g) => n + g.options.length, 0)} options
            </div>
          </div>
          <div style={{ marginLeft: "auto", textAlign: "right" }}>
            <div style={{ fontWeight: 650, fontVariantNumeric: "tabular-nums" }}>
              {formatBytes(totals.bytes)}
            </div>
            <div className="card-hint">{totals.files} files selected</div>
          </div>
        </header>

        {/* Step navigator */}
        <nav className="wizard-steps" aria-label="Install steps">
          {steps.map((g, i) => (
            <button
              key={`${g.index ?? "none"}-${g.label}`}
              className={`step ${i === stepIndex ? "active" : ""}`}
              onClick={() => setStepIndex(i)}
            >
              <span className="step-index">{g.index ?? "•"}</span>
              <span className="truncate">{g.label}</span>
              {stepResolved[i] && g.radioSets.length > 0 && (
                <span className="step-check">
                  <Icon.check size={14} />
                </span>
              )}
            </button>
          ))}
        </nav>

        {/* Step body */}
        <div className="wizard-body">
          {/* Why this update stopped to ask. Deliberately outside the step
              AnimatePresence: it applies to the whole install, not to whichever
              step is on screen, so it must not slide away when someone moves
              between steps to answer it. */}
          {carry && carryMessage(carry) && (
            <div
              className={carry.dropped.length > 0 ? "notice" : "notice info"}
              style={{ marginBottom: "var(--sp-4)" }}
            >
              {carryMessage(carry)}
            </div>
          )}

          <AnimatePresence mode="wait">
            <motion.div
              key={stepIndex}
              className="stack"
              initial={{ opacity: 0, x: 14 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -14 }}
              transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
            >
              {step && (
                <>
                  {/* Notices and forced/addon options that sit outside radio sets */}
                  {/* Cover art and warnings: full width, but height-capped. */}
                  {splitLoose(step).notices.map((o) => (
                    <div key={o.id} className="stack" style={{ gap: 0 }}>
                      {o.hasPreview && (
                        <PreviewHero
                          source={previewSource}
                          optionId={o.id}
                          alt={o.name}
                        />
                      )}
                      <div
                        className={`notice ${/warn/i.test(o.name) ? "" : "info"}`}
                        style={
                          o.hasPreview
                            ? { borderTopLeftRadius: 0, borderTopRightRadius: 0 }
                            : undefined
                        }
                      >
                        <span style={{ flexShrink: 0, marginTop: 2 }}>
                          {/warn/i.test(o.name) ? (
                            <Icon.warning size={17} />
                          ) : (
                            <Icon.info size={17} />
                          )}
                        </span>
                        <div style={{ minWidth: 0 }}>
                          <div className="notice-title">{o.name}</div>
                          {o.description && (
                            <div className="notice-body">{o.description}</div>
                          )}
                        </div>
                      </div>
                    </div>
                  ))}

                  {/* 1. Required files: always installed, shown first. */}
                  {splitLoose(step).required.length > 0 && (
                    <section className="option-set">
                      <div className="option-set-title">
                        <span>Required</span>
                        <span style={{ fontWeight: 400, textTransform: "none" }}>: always installed
                        </span>
                      </div>
                      <div className="option-grid">
                        {splitLoose(step).required.map((o) => (
                          <OptionCard
                            key={o.id}
                            option={o}
                            previewSource={previewSource}
                            kind="forced"
                            selected
                            onClick={() => undefined}
                          />
                        ))}
                      </div>
                    </section>
                  )}

                  {/* 2. One block per radio set: the base "pick one" choices. */}
                  {step.radioSets.map((key) => {
                    const members = step.options.filter((o) => o.radioSet === key);
                    const chosen = members.find((o) => selection.has(o.id));
                    return (
                      <section className="option-set" key={key}>
                        <div className="option-set-title">
                          <span>{setLabel(key)}</span>
                          <span style={{ fontWeight: 500, textTransform: "none" }}>: select one
                          </span>
                          {chosen ? (
                            <span style={{ marginLeft: "auto" }}>
                              <Chip kind="ok">
                                <Icon.check size={12} /> chosen
                              </Chip>
                            </span>
                          ) : (
                            <span style={{ marginLeft: "auto" }}>
                              <Chip kind="warn">not chosen</Chip>
                            </span>
                          )}
                        </div>
                        <div className="option-grid">
                          {members.map((o) => (
                            <OptionCard
                              key={o.id}
                              option={o}
                              previewSource={previewSource}
                              kind="radio"
                              selected={selection.has(o.id)}
                              onClick={() => pickRadio(step, o)}
                            />
                          ))}
                        </div>
                      </section>
                    );
                  })}

                  {/* 3. Addons last: they layer over whichever variant is chosen,
                         mirroring how the deployment engine orders them. */}
                  {splitLoose(step).addons.length > 0 && (
                    <section className="option-set">
                      <div className="option-set-title">
                        <span>Add-ons</span>
                        <span style={{ fontWeight: 400, textTransform: "none" }}>: optional, layered over the choice above
                        </span>
                      </div>
                      <div className="option-grid">
                        {splitLoose(step).addons.map((o) => (
                          <OptionCard
                            key={o.id}
                            option={o}
                            previewSource={previewSource}
                            kind="check"
                            selected={selection.has(o.id)}
                            onClick={() => toggleAddon(o)}
                          />
                        ))}
                      </div>
                    </section>
                  )}

                  {step.options.length === 0 && (
                    <div className="empty">
                      <span className="empty-icon">◎</span>
                      <div>This step has no options.</div>
                    </div>
                  )}
                </>
              )}
            </motion.div>
          </AnimatePresence>
        </div>

        <footer className="wizard-footer">
          <button
            className="btn ghost"
            onClick={onCancel}
            disabled={busy}
          >
            Cancel
          </button>
          <div style={{ flex: 1 }} />
          {unresolved > 0 && (
            <span className="card-hint">
              {unresolved} step{unresolved === 1 ? "" : "s"} without a choice
              (optional: those parts stay vanilla)
            </span>
          )}
          <button
            className="btn"
            disabled={stepIndex === 0 || busy}
            onClick={() => setStepIndex((i) => Math.max(0, i - 1))}
          >
            Back
          </button>
          {stepIndex < steps.length - 1 ? (
            <button
              className="btn primary"
              onClick={() => setStepIndex((i) => Math.min(steps.length - 1, i + 1))}
              disabled={busy}
            >
              Next
            </button>
          ) : (
            <button
              className="btn primary"
              onClick={() => onConfirm([...selection])}
              disabled={busy || totals.files === 0}
            >
              {busy ? "Working…" : `${confirmLabel} · ${totals.files} files`}
            </button>
          )}
        </footer>
      </motion.div>
    </motion.div>
  );
}

function OptionCard({
  option,
  previewSource,
  kind,
  selected,
  onClick,
}: {
  option: OptionView;
  previewSource: PreviewSource | null;
  kind: "radio" | "check" | "forced";
  selected: boolean;
  onClick: () => void;
}) {
  const locked = kind === "forced";
  return (
    <button
      type="button"
      role={kind === "radio" ? "radio" : "checkbox"}
      aria-checked={selected}
      aria-label={option.name}
      disabled={locked}
      className={`option ${selected ? "selected" : ""} ${locked ? "locked" : ""} ${
        option.hasPreview ? "has-thumb" : ""
      }`}
      onClick={onClick}
    >
      <OptionThumb
        source={previewSource}
        optionId={option.id}
        enabled={option.hasPreview}
        alt={option.name}
      />
      <span className={`mark ${kind === "radio" ? "radio" : "check"}`}>
        <AnimatePresence>
          {selected &&
            (kind === "radio" ? (
              <motion.span
                className="mark-inner"
                initial={{ scale: 0 }}
                animate={{ scale: 1 }}
                exit={{ scale: 0 }}
                transition={{ type: "spring", stiffness: 620, damping: 28 }}
              />
            ) : (
              <motion.span
                initial={{ scale: 0, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                exit={{ scale: 0, opacity: 0 }}
                transition={{ duration: 0.14 }}
                style={{ display: "grid", placeItems: "center" }}
              >
                <Icon.check size={13} />
              </motion.span>
            ))}
        </AnimatePresence>
      </span>
      <span style={{ minWidth: 0, flex: 1 }}>
        <span className="option-name" style={{ display: "block" }}>
          {option.name}
        </span>
        {option.description && (
          <span className="option-desc" style={{ display: "block" }}>
            {option.description}
          </span>
        )}
        <span className="option-meta" style={{ display: "block" }}>
          {locked && "required · "}
          {option.fileCount} file{option.fileCount === 1 ? "" : "s"} ·{" "}
          {formatBytes(option.sizeBytes)}
        </span>
      </span>
    </button>
  );
}
