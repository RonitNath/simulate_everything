import { Component, createMemo, onCleanup, onMount } from "solid-js";

interface ViewerCanvasProps {
  onEntityClick?: (entityId: number | null) => void;
}

const ViewerCanvas: Component<ViewerCanvasProps> = (props) => {
  const viewerSrc = createMemo(() => {
    const current = new URL(window.location.href);
    const viewer = new URL("/viewer/", window.location.origin);
    const server = current.searchParams.get("server");
    const ws = current.searchParams.get("ws");
    if (server) viewer.searchParams.set("server", server);
    if (ws) viewer.searchParams.set("ws", ws);
    return viewer.toString();
  });

  onMount(() => {
    const onMessage = (event: MessageEvent) => {
      if (event.origin !== window.location.origin) return;
      if (!event.data || typeof event.data !== "object") return;
      if (event.data.type !== "viewer-select-entity") return;
      const rawId = event.data.entityId;
      props.onEntityClick?.(typeof rawId === "number" ? rawId : null);
    };

    window.addEventListener("message", onMessage);
    onCleanup(() => window.removeEventListener("message", onMessage));
  });

  return (
    <iframe
      src={viewerSrc()}
      title="Simulate Everything Viewer"
      style={{
        width: "100%",
        height: "100%",
        border: "0",
        display: "block",
        background: "#090c12",
      }}
    />
  );
};

export default ViewerCanvas;
