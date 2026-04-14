import { render } from "solid-js/web";
import ReviewGallery from "./reviews/ReviewGallery";

const root = document.getElementById("app")!;
render(() => <ReviewGallery />, root);
