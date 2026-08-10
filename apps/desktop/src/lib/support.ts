/**
 * How to reach a person when the app cannot fix something itself.
 *
 * Kept in one place because it appears in two very different situations, and
 * they want different things from it. In Settings it is reference information —
 * findable when calm. On a failure it is a way out of a dead end, shown next to
 * the thing that just went wrong, because that is the moment someone actually
 * needs it and the moment they are least willing to go looking.
 */

export const SupportAddress = "support@apocryphamods.com";

/**
 * A mailto with the subject pre-filled, so a report arrives already sorted.
 *
 * Nothing about the machine is collected and nothing is sent anywhere: this only
 * opens the user's own mail client with a subject line. Anything more would be
 * telemetry, which this app does not do.
 */
export function supportMailto(about?: string): string {
  const subject = about ? `Apocrypha — ${about}` : "Apocrypha";
  return `mailto:${SupportAddress}?subject=${encodeURIComponent(subject)}`;
}
