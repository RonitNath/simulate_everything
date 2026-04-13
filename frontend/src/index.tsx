import { render } from "solid-js/web";
import App from "./App";
import LiveApp from "./LiveApp";
import ScoreboardApp from "./ScoreboardApp";

const root = document.getElementById("app")!;
const mode = (window as any).__MODE__;

if (mode === "live") {
  render(() => <LiveApp />, root);
} else if (mode === "scoreboard") {
  render(() => <ScoreboardApp />, root);
} else {
  render(() => <App />, root);
}
