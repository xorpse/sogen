#pragma once

#include "common.hpp"

#include <memory_manager.hpp>

namespace sogen::ffi
{
    class Arm64Machine
    {
      public:
        explicit Arm64Machine(std::unique_ptr<arm64_emulator> emulator);

        const Machine& machine() const;
        void start(uint64_t count) const;
        void map_memory(uint64_t address, uint64_t size, memory_permission permissions) const;
        bool unmap_memory(uint64_t address, uint64_t size) const;

      private:
        std::unique_ptr<arm64_emulator> emulator_{};
        std::unique_ptr<memory_manager> memory_{};
        Machine machine_;
    };

    backend_type default_arm64_backend();
    std::unique_ptr<Arm64Machine> create_arm64_machine(backend_type backend);
}
