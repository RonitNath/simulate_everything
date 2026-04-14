import { Component, For, Show, createSignal, onMount } from "solid-js";
import BundleCard, { BundleInfo } from "./BundleCard";
import * as css from "../styles/reviews.css";

const ReviewGallery: Component = () => {
  const [bundles, setBundles] = createSignal<BundleInfo[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  const [expandedId, setExpandedId] = createSignal<string | null>(null);

  async function fetchBundles() {
    setLoading(true);
    setError(null);
    try {
      const resp = await fetch("/api/v3/reviews/all");
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      const data: BundleInfo[] = await resp.json();
      setBundles(data);
    } catch (e: any) {
      setError(e.message ?? "failed to fetch bundles");
    } finally {
      setLoading(false);
    }
  }

  onMount(fetchBundles);

  function toggle(id: string) {
    setExpandedId((prev) => (prev === id ? null : id));
  }

  const behaviorBundles = () =>
    bundles().filter((b) => b.source === "behavior");
  const rrBundles = () =>
    bundles().filter((b) => b.source === "rr_capture");

  return (
    <div class={css.gallery}>
      <div class={css.header}>
        <div class={css.headerLeft}>
          <span class={css.title}>Review Gallery</span>
          <a href="/v3/rr" class={css.backLink}>
            ← Back to RR
          </a>
        </div>
        <button class={css.refreshBtn} onClick={fetchBundles}>
          Refresh
        </button>
      </div>
      <div class={css.content}>
        <Show when={loading()}>
          <div class={css.emptyState}>Loading...</div>
        </Show>
        <Show when={error()}>
          <div class={css.emptyState}>Error: {error()}</div>
        </Show>
        <Show when={!loading() && !error() && bundles().length === 0}>
          <div class={css.emptyState}>
            No review bundles found. Generate one with{" "}
            <code>./scripts/review-scenario.sh &lt;name&gt;</code>
          </div>
        </Show>
        <Show when={!loading() && !error() && bundles().length > 0}>
          <Show when={behaviorBundles().length > 0}>
            <div class={css.groupHeader}>Behavior Scenarios</div>
            <For each={behaviorBundles()}>
              {(bundle) => (
                <BundleCard
                  bundle={bundle}
                  expanded={expandedId() === bundle.id}
                  onToggle={() => toggle(bundle.id)}
                />
              )}
            </For>
          </Show>
          <Show when={rrBundles().length > 0}>
            <div class={css.groupHeader}>RR Captures</div>
            <For each={rrBundles()}>
              {(bundle) => (
                <BundleCard
                  bundle={bundle}
                  expanded={expandedId() === bundle.id}
                  onToggle={() => toggle(bundle.id)}
                />
              )}
            </For>
          </Show>
        </Show>
      </div>
    </div>
  );
};

export default ReviewGallery;
