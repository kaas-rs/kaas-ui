import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  Outlet,
  RouterProvider,
  createRootRoute,
  createRoute,
  createRouter,
  useParams,
} from "@tanstack/react-router";

import "./styles.css";
import { ClusterTabs, Shell } from "./shell";
import { Fleet } from "./pages/fleet";
import { CapabilitiesPage, ClusterConfigs, ClusterOverview } from "./pages/cluster";
import { TopicDetail, Topics } from "./pages/topics";
import { GroupDetail, Groups } from "./pages/groups";

const rootRoute = createRootRoute({ component: Shell });

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: Fleet,
});

/** Everything under a cluster carries the tab bar. */
const clusterRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/clusters/$clusterId",
  component: function ClusterLayout() {
    const { clusterId } = useParams({ from: "/clusters/$clusterId" });
    return (
      <>
        <ClusterTabs clusterId={clusterId} />
        <Outlet />
      </>
    );
  },
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
    groupsRoute,
    groupRoute,
    configsRoute,
    capabilitiesRoute,
  ]),
]);

const router = createRouter({ routeTree, defaultPreload: "intent" });

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
        <RouterProvider router={router} />
      </QueryClientProvider>
    </StrictMode>,
  );
}
