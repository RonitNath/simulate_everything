import { render } from "solid-js/web";
import App from "./App";
import LiveApp from "./LiveApp";
import ScoreboardApp from "./ScoreboardApp";
import V2App from "./V2App";
import V2SimApp from "./V2SimApp";
import V3App from "./V3App";
import V3ReplayApp from "./V3ReplayApp";
import V3DrillApp from "./V3DrillApp";

const root = document.getElementById("app")!;
const mode = (window as any).__MODE__;

if (mode === "live") {
  render(() => <LiveApp />, root);
} else if (mode === "scoreboard") {
  render(() => <ScoreboardApp />, root);
} else if (mode === "v3rr") {
  render(() => <V3App />, root);
} else if (mode === "v3replay") {
  render(() => <V3ReplayApp />, root);
} else if (mode === "v2rr") {
  render(() => <V2App />, root);
} else if (mode === "v2sim") {
  render(() => <V2SimApp />, root);
} else if (mode === "v3drill") {
  render(() => <V3DrillApp />, root);
} else {
  render(() => <App />, root);
}
