# Messages routes

- **`TailSpec::limit` over-fetches** — a partition's last chunk is kept whole,
  so `limit=20` comes back at or above 20 and the HTTP layer merges and
  truncates.
- **A filtered page's `limit` is a budget for records read, not matched.** The
  needle cannot go into kaas-lib's spec, which matches bytes, so a window is
  read and then reduced. `hasMore` says whether the *read* filled its budget
  and `nextOffset` comes off the window examined rather than off the surviving
  rows — a page that matched nothing still has to say where to look next, or
  paging stops on the first window a selective filter empties.
