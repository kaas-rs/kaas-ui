import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  Link,
  Outlet,
  RouterProvider,
  createRootRoute,
  createRoute,
  createRouter,
  useParams,
} from "@tanstack/react-router";

import "./styles.css";
import { PageTitle, Shell } from "@/shell";
import { Empty } from "@/components/domain";
import { BASE_PATH } from "@/api/base";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Fleet } from "@/pages/fleet";
import { CapabilitiesPage, ClusterConfigs, ClusterOverview } from "@/pages/cluster";
import { TopicDetail, Topics } from "@/pages/topics";
import { GroupDetail, Groups } from "@/pages/groups";
import { Messages } from "@/pages/messages";
import { messageSearchSchema } from "@/features/messages/search";

const rootRoute = createRootRoute({ component: Shell });

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: Fleet,
});

/** Everything under a cluster. Navigation lives in the sidebar, not in tabs. */
const clusterRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/clusters/$clusterId",
  component: Outlet,
});

const overviewRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "/",
  component: function Overview() {
    const { clusterId } = useParams({ from: "/clusters/$clusterId" });
    return <ClusterOverview clusterId={clusterId} />;
  },
});

const topicsRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "topics",
  component: function TopicsPage() {
    const { clusterId } = useParams({ from: "/clusters/$clusterId" });
    return <Topics clusterId={clusterId} />;
  },
});

const topicRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "topics/$topic",
  component: function TopicPage() {
    const { clusterId, topic } = useParams({
      from: "/clusters/$clusterId/topics/$topic",
    });
    return <TopicDetail clusterId={clusterId} topic={topic} />;
  },
});

/**
 * The message browser.
 *
 * Its own route rather than a tab inside the topic detail: the split pane
 * needs the full viewport height, and the search params below own the whole
 * page. That is what makes a seeked, filtered view with a message selected a
 * link someone can send.
 */
const messagesRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "topics/$topic/messages",
  validateSearch: messageSearchSchema,
  component: function MessagesPage() {
    const { clusterId, topic } = useParams({
      from: "/clusters/$clusterId/topics/$topic/messages",
    });
    return <Messages clusterId={clusterId} topic={topic} />;
  },
});

const groupsRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "groups",
  component: function GroupsPage() {
    const { clusterId } = useParams({ from: "/clusters/$clusterId" });
    return <Groups clusterId={clusterId} />;
  },
});

const groupRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "groups/$groupId",
  component: function GroupPage() {
    const { clusterId, groupId } = useParams({
      from: "/clusters/$clusterId/groups/$groupId",
    });
    return <GroupDetail clusterId={clusterId} groupId={groupId} />;
  },
});

const configsRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "configs",
  component: function ConfigsPage() {
    const { clusterId } = useParams({ from: "/clusters/$clusterId" });
    return <ClusterConfigs clusterId={clusterId} />;
  },
});

const capabilitiesRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "capabilities",
  component: function CapabilitiesRoute() {
    const { clusterId } = useParams({ from: "/clusters/$clusterId" });
    return <CapabilitiesPage clusterId={clusterId} />;
  },
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  clusterRoute.addChildren([
    overviewRoute,
    topicsRoute,
    topicRoute,
    messagesRoute,
    groupsRoute,
    groupRoute,
    configsRoute,
    capabilitiesRoute,
  ]),
]);

/**
 * What an unknown path renders.
 *
 * Needed because the server serves `index.html` for anything that is not a
 * file — that fallback is what makes a hard refresh on a deep link work, and
 * the cost is that a genuinely wrong URL reaches the router rather than a 404
 * page. Without this it reaches the router's bare default, outside the app
 * chrome, which reads as a broken build rather than a mistyped link.
 */
function NotFound() {
  return (
    <div className="p-6">
      <PageTitle
        title="No such page"
        subtitle="The address does not match anything in this application."
      />
      <Empty>
        <Link to="/" className="underline">
          Back to the fleet
        </Link>
      </Empty>
    </div>
  );
}

const router = createRouter({
  routeTree,
  defaultPreload: "intent",
  defaultNotFoundComponent: NotFound,
  // Where the server said this page is mounted, read from the `<base>` it
  // injected. Without it the first client-side navigation leaves the prefix
  // and lands on the domain root — which looks like the app working until you
  // click something, and is the confusing half of a base-path problem.
  basepath: BASE_PATH || "/",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // A cluster being unreachable is a fact to render, not a request to
      // retry three times before saying so.
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

const container = document.getElementById("root");
if (container) {
  createRoot(container).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={200}>
          <RouterProvider router={router} />
        </TooltipProvider>
      </QueryClientProvider>
    </StrictMode>,
  );
}
