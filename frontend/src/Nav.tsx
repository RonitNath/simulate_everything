import { Component } from "solid-js";
import * as styles from "./styles/app.css";

const LINKS = [
  { href: "/", label: "Simulator" },
  { href: "/rr", label: "Round Robin" },
  { href: "/live", label: "Live" },
  { href: "/scoreboard", label: "Scoreboard" },
];

const Nav: Component = () => {
  const current = (window as any).__PAGE__ || "simulator";

  return (
    <nav class={styles.nav}>
      {LINKS.map((link) => (
        <a
          href={link.href}
          class={styles.navLink}
          classList={{ [styles.navLinkActive]: link.label.toLowerCase().replace(" ", "") === current }}
        >
          {link.label}
        </a>
      ))}
    </nav>
  );
};

export default Nav;
