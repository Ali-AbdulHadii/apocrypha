/**
 * Startup screen.
 *
 * Shown while the first game and settings queries resolve. Deliberately quiet:
 * the lockup, and a single indeterminate bar. It fades out rather than cutting,
 * so the app does not flash.
 */

import { motion } from "framer-motion";
import { Lockup } from "./icons";

export function Splash() {
  return (
    <motion.div
      className="splash"
      initial={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.32, ease: [0.16, 1, 0.3, 1] }}
    >
      <div className="splash-inner">
        <motion.span
          className="splash-mark"
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5, ease: [0.16, 1, 0.3, 1] }}
        >
          <Lockup size={64} />
        </motion.span>

        <motion.div
          className="splash-bar"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.2 }}
        >
          <motion.span
            initial={{ x: "-100%" }}
            animate={{ x: "100%" }}
            transition={{ duration: 1.1, repeat: Infinity, ease: "easeInOut" }}
            style={{ width: "60%" }}
          />
        </motion.div>
      </div>
    </motion.div>
  );
}
