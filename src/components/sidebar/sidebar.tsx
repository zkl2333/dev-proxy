import { cn } from "@/lib/utils";
import { ScrollArea } from "../ui/scroll-area";
import { Button } from "../ui/button";
import { NavLink } from "react-router-dom";

interface SidebarProps extends React.HTMLAttributes<HTMLDivElement> {
  menu: { name: string; path: string }[];
}

export function Sidebar({ menu, className, ...rest }: SidebarProps) {
  return (
    <div className={cn(className)} {...rest}>
      <div className="h-full py-4 px-1">
        <ScrollArea>
          <nav className="space-y-2 p-1">
            {menu.map((item, index) => (
              <NavLink key={index} to={item.path} className="block">
                {({ isActive, isPending, isTransitioning }) => (
                  <Button
                    size={"lg"}
                    variant={isPending || isTransitioning || isActive ? "secondary" : "ghost"}
                    disabled={isPending || isTransitioning}
                    className="w-full justify-center"
                  >
                    {item.name}
                  </Button>
                )}
              </NavLink>
            ))}
          </nav>
        </ScrollArea>
      </div>
    </div>
  );
}
