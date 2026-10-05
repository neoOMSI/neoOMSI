import { PageHead } from "../components/ui";
import { docPath, url } from "../lib/routes";

export const NotFound = () => (
  <PageHead>
    <h1 className="display">Page not found</h1>
    <p className="mt-6 max-w-[34em] text-[19px] text-muted">
      This page does not exist. Try the{" "}
      <a className="link" href={url("/")}>
        home page
      </a>
      , the{" "}
      <a className="link" href={url(docPath("USER_GUIDE"))}>
        user guide
      </a>{" "}
      or the{" "}
      <a className="link" href={url("/faq/")}>
        FAQ
      </a>
      .
    </p>
  </PageHead>
);
