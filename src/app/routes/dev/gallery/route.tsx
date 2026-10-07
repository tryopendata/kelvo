import { AppComponentsSection } from "./_sections/app-components";
import { WidgetsCardsSection } from "./_sections/widgets-cards";
import { WidgetsChartsSection } from "./_sections/widgets-charts";

/**
 * Dev-only component gallery: every widget and app component with sample
 * props. The theme comes from `?theme=light|dark` (Playwright
 * screenshots both).
 */
export default function GalleryRoute() {
  return (
    <main className="flex flex-col gap-10 bg-background p-6 text-foreground">
      <h1 className="font-[590] text-[22px] tracking-[-0.022em]">Gallery</h1>
      <section aria-labelledby="g-widgets" className="flex flex-col gap-6">
        <h2 id="g-widgets" className="font-[590] text-[15px]">
          Widgets
        </h2>
        <WidgetsCardsSection />
      </section>
      <section aria-labelledby="g-charts" className="flex flex-col gap-6">
        <h2 id="g-charts" className="font-[590] text-[15px]">
          Charts
        </h2>
        <WidgetsChartsSection />
      </section>
      <section aria-labelledby="g-app" className="flex flex-col gap-6">
        <h2 id="g-app" className="font-[590] text-[15px]">
          App components
        </h2>
        <AppComponentsSection />
      </section>
    </main>
  );
}
