import type { EnvironmentSection } from "@/api/types"
import { RESOURCE_KINDS } from "@/components/domain"
import { SidebarMenuButton, SidebarMenuItem } from "@/components/ui/sidebar"

/**
 * One thing in the environment that is not a cluster.
 *
 * Rendered as text rather than as a button, on purpose: kaas-ui has no page
 * for a schema registry or an MQTT broker, and a nav row that does nothing on
 * click teaches people to stop trusting the rows that do. They are in the list
 * because "what is in staging" is a question the nav should answer, and the
 * answer is not only the brokers.
 *
 * It survives the collapse to icons. These are not targets, so the rail is not
 * strictly a set of targets any more — but a nav that answers "what is in
 * staging" only at full width answers it in the one state where the page had
 * room to answer it anyway. It keeps its glyph and gains the tooltip every
 * other collapsed row has.
 *
 * `SidebarMenuButton asChild` around a plain `div` is what buys that tooltip:
 * the styling and the collapsed-only tooltip come from the primitive, while
 * the element stays something that cannot be clicked, focused or announced as
 * a control. Hover is neutralised for the same reason.
 */
export function NavResourceItem({
  resource,
}: {
  resource: EnvironmentSection["resources"][number]
}) {
  const kind = RESOURCE_KINDS[resource.kind]
  const Icon = kind.icon

  return (
    <SidebarMenuItem>
      <SidebarMenuButton
        asChild
        // The endpoint and the kind, for the row that cannot lead anywhere.
        // "not probed" is the fleet card's job to say; a nav that repeated it
        // on every row would be nagging.
        tooltip={
          resource.endpoint
            ? `${resource.name} — ${resource.endpoint}`
            : resource.name
        }
        className="text-sidebar-foreground/55 hover:bg-transparent hover:text-sidebar-foreground/55"
      >
        <div
          title={
            resource.endpoint
              ? `${kind.label} — ${resource.endpoint}`
              : kind.label
          }
        >
          <Icon aria-hidden />
          <span className="truncate">{resource.name}</span>
        </div>
      </SidebarMenuButton>
    </SidebarMenuItem>
  )
}
