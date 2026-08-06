// The heading every page opens with.
//
// Its own file rather than a corner of the layout, because it is the one piece
// of that layout a *page* renders. The frame draws the sidebar and the bar;
// this is what the page itself says it is.

import type { ReactNode } from "react";

export function PageTitle({
  title,
  subtitle,
  actions,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className="mb-6 flex items-start justify-between gap-6">
      <div className="min-w-0">
        <h1 className="truncate text-[22px] font-semibold tracking-tight">{title}</h1>
        {subtitle ? (
          <div className="mt-1 text-[13px] text-ink-muted">{subtitle}</div>
        ) : null}
      </div>
      {actions ? <div className="flex items-center gap-3">{actions}</div> : null}
    </div>
  );
}
