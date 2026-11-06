// Temporary page for ecosystem sections whose phase hasn't shipped yet, so the
// new navigation is complete while each module is built.

import { Empty, PageHead } from "../components/ui";

export function Coming({ eyebrow, title, desc, phase }: { eyebrow: string; title: string; desc: string; phase: string }) {
  return (
    <div className="page">
      <PageHead eyebrow={eyebrow} title={title} desc={desc} />
      <div className="panel">
        <Empty title={`Arriving in ${phase}`}>This part of the Nocturn ecosystem is being built. Everything in System keeps working as before.</Empty>
      </div>
    </div>
  );
}
