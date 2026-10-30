#include "arm64.hpp"

#include <sogen-sys/arm64.rs.h>

#include <stdexcept>
#include <string>

namespace sogen::ffi
{
    Arm64Machine::Arm64Machine(std::unique_ptr<arm64_emulator> emulator)
        : emulator_(std::move(emulator)),
          memory_(std::make_unique<memory_manager>(*this->emulator_)),
          machine_(
              *this->emulator_, [this]() -> guest_cpu& { return this->emulator_->get_cpu(0); },
              [](cpu_interface&, const std::function<void()>& callback) { callback(); })
    {
    }

    const Machine& Arm64Machine::machine() const
    {
        return this->machine_;
    }

    void Arm64Machine::start(const uint64_t count) const
    {
        this->emulator_->start(static_cast<size_t>(count));
    }

    void Arm64Machine::map_memory(const uint64_t address, const uint64_t size, const memory_permission permissions) const
    {
        if (!this->memory_->allocate_memory(address, static_cast<size_t>(size), permissions))
        {
            throw std::runtime_error("Cannot allocate ARM64 guest memory at " + std::to_string(address));
        }
    }

    bool Arm64Machine::unmap_memory(const uint64_t address, const uint64_t size) const
    {
        return this->memory_->release_memory(address, static_cast<size_t>(size));
    }

    backend_type default_arm64_backend()
    {
        return get_arm64_emulator_backend_from_environment();
    }

    std::unique_ptr<Arm64Machine> create_arm64_machine(const backend_type backend)
    {
        return std::make_unique<Arm64Machine>(create_arm64_emulator(backend));
    }
}
