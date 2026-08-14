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
import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  formatBytes,
  type CarryView,
  type GroupView,
  type ReplaceCandidateView,
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
  /**
   * The installed mod this archive appears to be a new version of. An uncertain
   * candidate is offered as a choice; a certain one is stated, because it was
   * not in doubt and the install is going to act on it either way.
   */
  replaces?: ReplaceCandidateView | null;
  /**
   * The archive being installed, when there is one. Required for a conditional
   * installer, whose steps have to be re-derived from the engine as choices are
   * made; without it the wizard shows the installer as first analysed.
   */
  source?: { gameId: string; archivePath: string } | null;
  onCancel: () => void;
  /** `replaces` is the mod id to update in place, or null to add a new mod. */
  onConfirm: (selection: string[], replaces: string | null) => void;
  confirmLabel?: string;
}

/** Whether two selections hold the same ids, regardless of order. */
function sameSelection(a: Set<string>, b: string[]): boolean {
  return a.size === b.length && b.every((id) => a.has(id));
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
  replaces,
  source,
  onCancel,
  onConfirm,
  confirmLabel = "Install",
}: Props) {
  const [selection, setSelection] = useState<Set<string>>(
    () => new Set(mod.selection),
  );
  const [stepIndex, setStepIndex] = useState(0);
  /**
   * The installer as it currently stands.
   *
   * For every format but one this is just `mod`, unchanged for the life of the
   * dialog: the options an archive offers do not depend on which of them you
   * pick. A FOMOD is the exception — a step can exist only because of an answer
   * given two steps earlier — so its view is re-derived by the engine after
   * each change rather than filtered here. Deciding it in the interface would
   * put a second implementation of the author's conditions in the one place
   * least able to be tested.
   */
  const [view, setView] = useState<ModView>(mod);
  const conditional = mod.installerModel === "Fomod" && !!source;
  /** Guards against an older answer landing after a newer one. */
  const evaluation = useRef(0);

  useEffect(() => setView(mod), [mod]);

  useEffect(() => {
    if (!conditional || !source) return;
    const mine = ++evaluation.current;
    api
      .evaluateSelection(source.gameId, source.archivePath, [...selection])
      .then((next) => {
        if (mine !== evaluation.current) return;
        setView(next);
        // The engine's answer includes what conditions force and excludes what
        // they forbid, so it is the selection, not a suggestion. Only applied
        // when it actually differs, or this would re-trigger itself forever.
        if (!sameSelection(selection, next.selection)) {
          setSelection(new Set(next.selection));
        }
      })
      .catch(() => {
        // A refusal (an installer whose conditions do not settle) surfaces on
        // Install, where it can be explained. Leaving the last good view up is
        // better than blanking the dialog mid-click.
      });
  }, [conditional, source, selection]);
  /**
   * Whether to update the mod this looks like a new version of, or add it
   * alongside.
   *
   * Starts on "replace" because that is what an archive matching an installed
   * mod usually is, and because the alternative — two rows of the same mod, both
   * enabled, contesting every file — is the outcome nobody wants by accident.
   */
  const [replaceExisting, setReplaceExisting] = useState(true);

  const steps = view.groups;
  // A conditional installer can drop the step being looked at, when the answer
  // that made it visible is withdrawn.
  const step = steps[Math.min(stepIndex, Math.max(steps.length - 1, 0))];

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

  /**
   * A step is "resolved" once every choice it insists on has been made.
   *
   * Every radio set must have an answer, as before. A group whose author
   * declared that at least one option must be chosen counts too: that is a
   * distinction only a manifest can draw, and it is the difference between
   * "you may keep vanilla" and "this installer will not proceed".
   */
  const stepResolved = useMemo(() => {
    return steps.map((g) => {
      const radiosAnswered = g.radioSets.every((key) =>
        g.options.some((o) => o.radioSet === key && selection.has(o.id)),
      );
      const mustChoose =
        g.cardinality === "select-exactly-one" ||
        g.cardinality === "select-at-least-one";
      const choosable = g.options.filter((o) => o.selectMode !== "info");
      const answered =
        !mustChoose ||
        choosable.length === 0 ||
        choosable.some((o) => selection.has(o.id));
      return radiosAnswered && answered;
    });
  }, [steps, selection]);

  const chosenOptions = useMemo(
    () => steps.flatMap((g) => g.options).filter((o) => selection.has(o.id)),
    [steps, selection],
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
              {steps.reduce((n, g) => n + g.options.length, 0)} options
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
          {/* The identity question, when there is one. It sits above the carry
              banner because it is the larger of the two: which mod this is
              decides what carrying even means. */}
          {replaces && !replaces.certain && (
            <div className="notice info" style={{ marginBottom: "var(--sp-4)" }}>
              <div>
                This looks like a new version of{" "}
                <strong>{replaces.name}</strong>
                {replaces.version ? ` ${replaces.version}` : ""}, but only the
                name says so.
              </div>
              <div
                className="row"
                style={{ gap: "var(--sp-2)", marginTop: "var(--sp-3)" }}
                role="radiogroup"
                aria-label="What to do with the installed mod"
              >
                <button
                  type="button"
                  role="radio"
                  aria-checked={replaceExisting}
                  className={`btn ${replaceExisting ? "primary" : ""}`}
                  onClick={() => setReplaceExisting(true)}
                >
                  Replace it
                </button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={!replaceExisting}
                  className={`btn ${replaceExisting ? "" : "primary"}`}
                  onClick={() => setReplaceExisting(false)}
                >
                  Add as a separate mod
                </button>
              </div>
            </div>
          )}

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

          {/* What the installer asked for that could not be honoured exactly.
              Outside the step animation for the same reason as the carry
              banner: it is about the whole install, not one page of it. */}
          {(view.warnings ?? []).map((warning) => (
            <div
              key={warning}
              className="notice"
              style={{ marginBottom: "var(--sp-4)" }}
            >
              {warning}
            </div>
          ))}

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
                          {/* An option that cannot be chosen says why here.
                              Showing it disabled with its reason is the point:
                              a choice that silently disappeared would read as
                              the manager losing it. */}
                          {o.blockedReason && (
                            <div className="notice-body">{o.blockedReason}</div>
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
              onClick={() =>
                onConfirm(
                  [...selection],
                  // A certain match replaces without being asked about; an
                  // uncertain one only if that is what was chosen above.
                  replaces && (replaces.certain || replaceExisting)
                    ? replaces.modId
                    : null,
                )
              }
              disabled={busy || totals.files === 0}
            >
              {busy
                ? "Working…"
                : `${replaces && (replaces.certain || replaceExisting) ? "Update" : confirmLabel} · ${totals.files} files`}
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
