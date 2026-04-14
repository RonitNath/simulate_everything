import { Component, Show } from "solid-js";
import * as css from "../styles/reviews.css";

export interface InvariantInfo {
  name: string;
  passed: boolean;
}

export interface BundleInfo {
  id: string;
  source: string;
  name: string;
  category: string;
  passed?: boolean;
  invariants?: InvariantInfo[];
  tick_count: number;
  agent_names: string[];
  annotation?: string;
  seed?: number;
  has_filmstrip: boolean;
  has_review_html: boolean;
  modified_at: number;
}

interface BundleCardProps {
  bundle: BundleInfo;
  expanded: boolean;
  onToggle: () => void;
}

const BundleCard: Component<BundleCardProps> = (props) => {
  const passCount = () =>
    props.bundle.invariants?.filter((i) => i.passed).length ?? 0;
  const totalCount = () => props.bundle.invariants?.length ?? 0;

  return (
    <div class={css.card}>
      <div class={css.cardHeader} onClick={props.onToggle}>
        <span class={css.cardChevron}>{props.expanded ? "▼" : "▶"}</span>
        <div class={css.cardInfo}>
          <div class={css.cardTitle}>{props.bundle.name}</div>
          <div class={css.cardMeta}>
            <Show when={props.bundle.agent_names.length > 0}>
              <span>{props.bundle.agent_names.join(", ")}</span>
            </Show>
            <span>{props.bundle.tick_count} ticks</span>
            <Show when={props.bundle.invariants}>
              <span
                class={`${css.badge} ${
                  props.bundle.passed ? css.badgePass : css.badgeFail
                }`}
              >
                {props.bundle.passed ? "✓" : "✗"} {passCount()}/{totalCount()}
              </span>
            </Show>
            <Show when={!props.bundle.has_review_html}>
              <span class={css.badge}>no review.html</span>
            </Show>
          </div>
          <Show when={props.bundle.annotation}>
            <div class={css.annotation}>"{props.bundle.annotation}"</div>
          </Show>
          <Show when={!props.expanded && props.bundle.has_filmstrip}>
            <img
              class={css.filmstrip}
              src={`/reviews/files/${props.bundle.id}/filmstrip.png`}
              alt="filmstrip"
              loading="lazy"
            />
          </Show>
        </div>
      </div>
      <Show when={props.expanded}>
        <Show
          when={props.bundle.has_review_html}
          fallback={
            <div class={css.noReviewHtml}>
              No review.html available for this bundle. Run{" "}
              <code>./scripts/build-review-bundle.sh</code> to generate it.
            </div>
          }
        >
          <div class={css.iframeContainer}>
            <iframe
              class={css.iframe}
              src={`/reviews/files/${props.bundle.id}/review.html`}
            />
          </div>
        </Show>
      </Show>
    </div>
  );
};

export default BundleCard;
