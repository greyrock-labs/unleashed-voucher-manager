"use client";

import Spinner from "@/components/utils/Spinner";
import VoucherCard from "@/components/VoucherCard";
import VoucherModal from "@/components/modals/VoucherModal";
import { GuestPass } from "@/types/voucher";
import { api } from "@/utils/api";
import { notify } from "@/utils/notifications";
import { useMemo, useEffect, useCallback, useState } from "react";

export default function VouchersTab() {
  const [loading, setLoading] = useState(true);
  const [passes, setPasses] = useState<GuestPass[]>([]);
  const [viewPass, setViewPass] = useState<GuestPass | null>(null);
  const [searchQuery, setSearchQuery] = useState("");

  const filteredPasses = useMemo(() => {
    if (!searchQuery.trim()) return passes;

    const query = searchQuery.toLowerCase().trim();
    return passes.filter((pass) => pass.name?.toLowerCase().includes(query));
  }, [passes, searchQuery]);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const res = await api.getAllPasses();
      setPasses(res || []);
    } catch {
      notify("Failed to load guest passes", "error");
    }
    setLoading(false);
  }, []);

  const closeModal = useCallback(() => {
    setViewPass(null);
  }, []);

  useEffect(() => {
    load();
    window.addEventListener("vouchersUpdated", load);

    return () => {
      window.removeEventListener("vouchersUpdated", load);
    };
  }, [load]);

  return (
    <div className="flex-1">
      <div className="mb-2">
        <div className="relative">
          <input
            type="text"
            placeholder="Search guest passes by name..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery("")}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-secondary text-2xl hover:text-primary"
            >
              &times;
            </button>
          )}
        </div>
      </div>
      <div className="mb-4 flex flex-wrap items-center gap-3">
        <button onClick={load} className="btn-secondary">
          Refresh
        </button>
      </div>

      {searchQuery && (
        <div className="mb-4 text-sm text-secondary">
          Showing {filteredPasses.length} of {passes.length} guest passes
        </div>
      )}

      {loading ? (
        <Spinner />
      ) : !filteredPasses.length ? (
        <div className="text-center py-8 text-secondary">
          {searchQuery
            ? "No guest passes found matching your search"
            : "No guest passes found"}
        </div>
      ) : (
        <div className="grid gap-4 grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
          {filteredPasses.map((p) => (
            <VoucherCard key={p.id} pass={p} onClick={setViewPass} />
          ))}
        </div>
      )}

      {viewPass && <VoucherModal pass={viewPass} onClose={closeModal} />}
    </div>
  );
}
