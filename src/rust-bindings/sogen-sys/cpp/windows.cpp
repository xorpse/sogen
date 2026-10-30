#include "windows.hpp"

#include <sogen-sys/windows.rs.h>

#include <emulator_utils.hpp>
#include <inherited_pipes.hpp>
#include <minidump_loader.hpp>
#include <network/socket_factory.hpp>
#include <registry/registry_file.hpp>
#include <snapshot.hpp>
#include <utils/finally.hpp>

#include <sys/stat.h>

#ifndef STATUS_DIRECTORY_NOT_EMPTY
#define STATUS_DIRECTORY_NOT_EMPTY ((NTSTATUS)0xC0000101L)
#endif

#include <algorithm>
#include <charconv>
#include <cstring>
#include <stdexcept>
#include <string>
#include <utility>

namespace sogen::ffi
{
    namespace
    {
        static_assert(static_cast<size_t>(Event::SectionFirstExecution) + 1 == event_count);

        static_assert(static_cast<uint16_t>(HandleType::Reserved) == handle_types::reserved);
        static_assert(static_cast<uint16_t>(HandleType::File) == handle_types::file);
        static_assert(static_cast<uint16_t>(HandleType::Device) == handle_types::device);
        static_assert(static_cast<uint16_t>(HandleType::Event) == handle_types::event);
        static_assert(static_cast<uint16_t>(HandleType::Section) == handle_types::section);
        static_assert(static_cast<uint16_t>(HandleType::Symlink) == handle_types::symlink);
        static_assert(static_cast<uint16_t>(HandleType::Directory) == handle_types::directory);
        static_assert(static_cast<uint16_t>(HandleType::Semaphore) == handle_types::semaphore);
        static_assert(static_cast<uint16_t>(HandleType::Port) == handle_types::port);
        static_assert(static_cast<uint16_t>(HandleType::Thread) == handle_types::thread);
        static_assert(static_cast<uint16_t>(HandleType::Registry) == handle_types::registry);
        static_assert(static_cast<uint16_t>(HandleType::Mutant) == handle_types::mutant);
        static_assert(static_cast<uint16_t>(HandleType::Token) == handle_types::token);
        static_assert(static_cast<uint16_t>(HandleType::Window) == handle_types::window);
        static_assert(static_cast<uint16_t>(HandleType::Menu) == handle_types::menu);
        static_assert(static_cast<uint16_t>(HandleType::Timer) == handle_types::timer);
        static_assert(static_cast<uint16_t>(HandleType::UserTimer) == handle_types::user_timer);
        static_assert(static_cast<uint16_t>(HandleType::Monitor) == handle_types::monitor);
        static_assert(static_cast<uint16_t>(HandleType::Desktop) == handle_types::desktop);
        static_assert(static_cast<uint16_t>(HandleType::IoCompletion) == handle_types::io_completion);
        static_assert(static_cast<uint16_t>(HandleType::WaitCompletionPacket) == handle_types::wait_completion_packet);
        static_assert(static_cast<uint16_t>(HandleType::WorkerFactory) == handle_types::worker_factory);
        static_assert(static_cast<uint16_t>(HandleType::PrivateNamespace) == handle_types::private_namespace);
        static_assert(static_cast<uint16_t>(HandleType::Process) == handle_types::process);
        static_assert(static_cast<uint16_t>(HandleType::AcceleratorTable) == handle_types::accelerator_table);
        static_assert(static_cast<uint16_t>(HandleType::ManagedThread) == handle_types::managed_thread);
        static_assert(static_cast<uint16_t>(HandleType::Job) == handle_types::job);
        static_assert(static_cast<uint16_t>(HandleType::DeferredWindowPositions) == handle_types::deferred_window_positions);

        std::string_view to_view(const ::rust::Str text)
        {
            return {text.data(), text.size()};
        }

        std::string_view to_view(const ::rust::String& text)
        {
            return {text.data(), text.size()};
        }

        std::filesystem::path to_path(const std::string_view text)
        {
            return {std::u8string_view(reinterpret_cast<const char8_t*>(text.data()), text.size())};
        }

        std::filesystem::path to_path(const ::rust::String& text)
        {
            return to_path(to_view(text));
        }

        std::filesystem::path to_path(const ::rust::Str text)
        {
            return to_path(to_view(text));
        }

        windows_path to_windows_path(const ::rust::String& text)
        {
            if (text.empty())
            {
                return {};
            }

            return windows_path{to_path(text)};
        }

        windows_path to_windows_path(const ::rust::Str text)
        {
            return windows_path{to_path(text)};
        }

        std::vector<std::byte> to_bytes(const ::rust::Slice<const uint8_t> data)
        {
            const auto bytes = std::as_bytes(std::span(data.data(), data.size()));
            return {bytes.begin(), bytes.end()};
        }

        ::rust::Vec<uint8_t> to_rust_bytes(const std::span<const std::byte> data)
        {
            ::rust::Vec<uint8_t> result{};
            result.reserve(data.size());
            for (const auto value : data)
            {
                result.push_back(static_cast<uint8_t>(value));
            }

            return result;
        }

        uint64_t remaining_nanoseconds(const std::chrono::steady_clock::time_point due, const std::chrono::steady_clock::time_point now)
        {
            if (due <= now)
            {
                return 0;
            }

            return static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(due - now).count());
        }

        int64_t to_nanoseconds(const timespec& time)
        {
            return static_cast<int64_t>(time.tv_sec) * 1'000'000'000 + static_cast<int64_t>(time.tv_nsec);
        }

        FileStat make_file_stat(const struct compat_stat& stat)
        {
            return FileStat{
                .is_directory = (stat.st_mode & S_IFMT) == S_IFDIR,
                .size = static_cast<uint64_t>(std::max<int64_t>(stat.st_size, 0)),
                .accessed_nanoseconds = to_nanoseconds(stat.st_atimespec),
                .modified_nanoseconds = to_nanoseconds(stat.st_mtimespec),
                .changed_nanoseconds = to_nanoseconds(stat.st_ctimespec),
            };
        }

        // Without an emulation root the path is empty, which absolute() rejects on Windows.
        std::string describe_path(const std::filesystem::path& path)
        {
            return path.empty() ? std::string{} : std::filesystem::absolute(path).string();
        }

        std::string describe_mount(const mount_spec& mount)
        {
            return "mount=" + render_mount_spec(mount) + "\n";
        }

        std::string describe_configuration(const emulator_settings& settings)
        {
            std::string configuration{};
            configuration += "emulation-root=" + describe_path(settings.emulation_root) + "\n";
            configuration += "root-mode=" + std::string{to_string(settings.root_mode)} + "\n";
            configuration += "registry=" + describe_path(settings.registry_directory) + "\n";
            configuration += "network=" + std::string{network::to_string(settings.network)} + "\n";
            configuration += std::string{"spoof-gpu-features="} + (settings.spoof_gpu_features ? "on" : "off") + "\n";

            for (const auto& mount : settings.mounts)
            {
                configuration += describe_mount(mount);
            }

            return configuration;
        }

        mount_spec make_mount_spec(const MountSpec& mount)
        {
            return mount_spec{
                .host_path = to_path(mount.host_path),
                .guest_path = to_windows_path(mount.guest_path),
                .mode = mount.mode,
            };
        }

        ::rust::String from_utf8(const std::string_view text)
        {
            return ::rust::String::lossy(text.data(), text.size());
        }

        ::rust::String from_utf16(const std::u16string_view text)
        {
            return ::rust::String::lossy(text.data(), text.size());
        }

        ::rust::String from_path(const std::filesystem::path& path)
        {
            const auto text = path.u8string();
            return ::rust::String::lossy(reinterpret_cast<const char*>(text.data()), text.size());
        }

        emulator_settings make_emulator_settings(const EmulatorSettings& source)
        {
            emulator_settings settings{};
            settings.disable_logging = !source.logging;
            settings.use_relative_time = source.use_relative_time;
            settings.use_instruction_precision = source.use_instruction_precision;
            settings.intercept_rdtsc = source.intercept_rdtsc;
            settings.intercept_kusd = source.intercept_kusd;
            settings.emulation_root = to_path(source.emulation_root);
            settings.registry_directory = to_path(source.registry_directory);
            settings.load_registry = source.load_registry;
            settings.root_mode = source.root_mode;
            settings.fake_env.number_of_processors = source.number_of_processors;
            settings.fake_env.nt_product_type = source.nt_product_type;
            settings.network = source.network;
            settings.spoof_gpu_features = source.spoof_gpu_features;

            for (const auto& mapping : source.port_mappings)
            {
                settings.port_mappings.insert_or_assign(mapping.emulator_port, mapping.host_port);
            }

            for (const auto& mount : source.mounts)
            {
                settings.mounts.push_back(make_mount_spec(mount));
            }

            return settings;
        }

        application_settings make_application_settings(const ApplicationSettings& source)
        {
            application_settings settings{};
            settings.application = to_windows_path(source.application);
            settings.argument0 = u8_to_u16(to_view(source.argument0));
            settings.working_directory = to_windows_path(source.working_directory);

            settings.arguments.reserve(source.arguments.size());
            for (const auto& argument : source.arguments)
            {
                settings.arguments.push_back(u8_to_u16(to_view(argument)));
            }

            if (source.has_command_line)
            {
                settings.command_line = u8_to_u16(to_view(source.command_line));
            }

            for (const auto& variable : source.environment)
            {
                settings.environment.insert_or_assign(u8_to_u16(to_view(variable.name)), u8_to_u16(to_view(variable.value)));
            }

            if (source.has_process_id)
            {
                settings.process_id = source.process_id;
            }

            if (source.has_thread_id)
            {
                settings.thread_id = source.thread_id;
            }

            return settings;
        }

        network::address to_network_address(const IpAddress& ip)
        {
            network::address address{};
            if (ip.ipv6)
            {
                in6_addr ipv6{};
                std::memcpy(&ipv6, ip.octets.data(), sizeof(ipv6));
                address.set_ipv6(ipv6);
            }
            else
            {
                in_addr ipv4{};
                std::memcpy(&ipv4, ip.octets.data(), sizeof(ipv4));
                address.set_ipv4(ipv4);
            }

            return address;
        }

        network::address to_network_address(const SocketAddress& source)
        {
            auto address = to_network_address(source.ip);
            if (source.ip.ipv6)
            {
                address.get_in6_addr().sin6_flowinfo = source.flow_info;
                address.get_in6_addr().sin6_scope_id = source.scope_id;
            }

            address.set_port(source.port);
            return address;
        }

        SocketAddress to_socket_address(const network::address& address)
        {
            SocketAddress result{};
            result.port = address.get_port();
            if (address.is_ipv6())
            {
                const auto& ipv6 = address.get_in6_addr();
                result.ip.ipv6 = true;
                std::memcpy(result.ip.octets.data(), &ipv6.sin6_addr, sizeof(ipv6.sin6_addr));
                result.flow_info = ipv6.sin6_flowinfo;
                result.scope_id = ipv6.sin6_scope_id;
            }
            else
            {
                const auto& ipv4 = address.get_in_addr();
                std::memcpy(result.ip.octets.data(), &ipv4.sin_addr, sizeof(ipv4.sin_addr));
            }

            return result;
        }

        ::rust::Slice<uint8_t> to_mutable_slice(const std::span<std::byte> data)
        {
            return {reinterpret_cast<uint8_t*>(data.data()), data.size()};
        }

        class rust_dns_lookup : public network::dns_lookup
        {
          public:
            rust_dns_lookup(::rust::Box<DnsResolver> resolver, std::unique_ptr<network::dns_lookup> fallback)
                : resolver_(std::move(resolver)),
                  fallback_(std::move(fallback))
            {
            }

            std::vector<network::address> resolve_host(const std::string_view hostname, const std::optional<int> family) override
            {
                uint8_t requested_family = 0;
                if (family == AF_INET)
                {
                    requested_family = 4;
                }
                else if (family == AF_INET6)
                {
                    requested_family = 6;
                }

                const auto answer = this->resolver_->resolve(to_slice(as_bytes(hostname)), requested_family);
                if (answer.defer)
                {
                    return this->fallback_->resolve_host(hostname, family);
                }

                std::vector<network::address> results{};
                results.reserve(answer.addresses.size());
                for (const auto& entry : answer.addresses)
                {
                    results.push_back(to_network_address(entry));
                }

                return results;
            }

          private:
            ::rust::Box<DnsResolver> resolver_;
            std::unique_ptr<network::dns_lookup> fallback_;
        };

        class rust_clock : public utils::clock
        {
          public:
            explicit rust_clock(::rust::Box<Clock> clock)
                : clock_(std::move(clock))
            {
            }

            // The system_clock period differs between MSVC, libc++ and libstdc++.
            system_time_point system_now() override
            {
                const std::chrono::nanoseconds since_epoch{this->clock_->system_time_nanoseconds()};
                return system_time_point{std::chrono::duration_cast<system_duration>(since_epoch)};
            }

            steady_time_point steady_now() override
            {
                const std::chrono::duration<uint64_t, std::nano> since_start{this->clock_->steady_time_nanoseconds()};
                return steady_time_point{std::chrono::duration_cast<steady_duration>(since_start)};
            }

            uint64_t timestamp_counter() override
            {
                return this->clock_->timestamp_counter();
            }

          private:
            ::rust::Box<Clock> clock_;
        };

        int to_socket_error(const SocketStatus& status)
        {
            switch (status.error)
            {
            case SocketErrorKind::Success:
                return 0;
            case SocketErrorKind::Os:
                return status.os_error;
            case SocketErrorKind::WouldBlock:
                return SERR(EWOULDBLOCK);
            case SocketErrorKind::ConnectionRefused:
                return SERR(ECONNREFUSED);
            case SocketErrorKind::ConnectionReset:
                return SERR(ECONNRESET);
            case SocketErrorKind::AddressInUse:
                return SERR(EADDRINUSE);
            case SocketErrorKind::AddressNotAvailable:
                return SERR(EADDRNOTAVAIL);
            case SocketErrorKind::NotConnected:
                return SERR(ENOTCONN);
            case SocketErrorKind::InvalidInput:
                return SERR(EINVAL);
            default:
                return SOCKET_ERROR;
            }
        }

        class rust_socket : public network::i_socket
        {
          public:
            explicit rust_socket(::rust::Box<Socket> socket)
                : socket_(std::move(socket))
            {
            }

            // Rust sockets never block; AFD implements blocking guest calls by retrying once the socket is ready.
            void set_blocking(const bool /*blocking*/) override
            {
            }

            int get_last_error() override
            {
                return this->last_error_;
            }

            std::optional<int> get_socket_error() override
            {
                SocketStatus pending{};
                if (!this->socket_->take_error(pending))
                {
                    return std::nullopt;
                }

                return to_socket_error(pending);
            }

            bool is_ready(const bool in_poll) override
            {
                const auto readiness = this->socket_->readiness();
                return readiness.error || readiness.hang_up || (in_poll ? readiness.readable : readiness.writable);
            }

            bool is_listening() override
            {
                return this->socket_->is_listening();
            }

            std::optional<network::address> get_local_address() override
            {
                SocketAddress address{};
                if (!this->socket_->local_address(address))
                {
                    return std::nullopt;
                }

                return to_network_address(address);
            }

            bool bind(const network::address& address) override
            {
                return this->succeeded(this->socket_->bind(to_socket_address(address)));
            }

            bool connect(const network::address& address) override
            {
                return this->succeeded(this->socket_->connect(to_socket_address(address)));
            }

            bool listen(const int backlog) override
            {
                return this->succeeded(this->socket_->listen(static_cast<uint32_t>(std::max(backlog, 0))));
            }

            std::unique_ptr<network::i_socket> accept(network::address& address) override
            {
                SocketAddress peer{};
                SocketStatus status{};
                auto accepted = this->socket_->accept(peer, status);
                if (!this->succeeded(status))
                {
                    return nullptr;
                }

                address = to_network_address(peer);
                return std::make_unique<rust_socket>(std::move(accepted));
            }

            sent_size send(const std::span<const std::byte> data) override
            {
                return this->transferred(this->socket_->send(to_slice(data)));
            }

            sent_size sendto(const network::address& destination, const std::span<const std::byte> data) override
            {
                return this->transferred(this->socket_->send_to(to_slice(data), to_socket_address(destination)));
            }

            sent_size recv(const std::span<std::byte> data) override
            {
                return this->transferred(this->socket_->receive(to_mutable_slice(data)));
            }

            sent_size recvfrom(network::address& source, const std::span<std::byte> data) override
            {
                SocketAddress origin{};
                const auto received = this->transferred(this->socket_->receive_from(to_mutable_slice(data), origin));
                if (received >= 0)
                {
                    source = to_network_address(origin);
                }

                return received;
            }

            SocketReadiness readiness()
            {
                return this->socket_->readiness();
            }

          private:
            ::rust::Box<Socket> socket_;
            int last_error_{};

            bool succeeded(const SocketStatus& status)
            {
                this->last_error_ = to_socket_error(status);
                return status.error == SocketErrorKind::Success;
            }

            sent_size transferred(const SocketStatus& status)
            {
                return this->succeeded(status) ? static_cast<sent_size>(status.count) : -1;
            }
        };

        int16_t to_poll_events(const SocketReadiness& readiness, const int16_t requested)
        {
            int events = 0;
            if (readiness.readable)
            {
                events |= requested & (POLLIN | POLLRDNORM | POLLRDBAND);
            }

            if (readiness.writable)
            {
                events |= requested & (POLLOUT | POLLWRNORM);
            }

            if (readiness.hang_up)
            {
                events |= POLLHUP;
            }

            if (readiness.error)
            {
                events |= POLLERR;
            }

            return static_cast<int16_t>(events);
        }

        class rust_socket_factory : public network::socket_factory
        {
          public:
            rust_socket_factory(::rust::Box<SocketFactory> factory, std::unique_ptr<network::socket_factory> fallback)
                : factory_(std::move(factory)),
                  fallback_(std::move(fallback))
            {
            }

            std::unique_ptr<network::i_socket> create_socket(const int af, const int type, const int protocol) override
            {
                if ((af == AF_INET || af == AF_INET6) && (type == SOCK_STREAM || type == SOCK_DGRAM))
                {
                    auto socket = this->factory_->create_socket(af == AF_INET6 ? 6 : 4,
                                                                type == SOCK_STREAM ? SocketType::Stream : SocketType::Datagram);
                    if (socket->is_active())
                    {
                        return std::make_unique<rust_socket>(std::move(socket));
                    }
                }

                return this->fallback_->create_socket(af, type, protocol);
            }

            // The fallback factory only accepts its own sockets, so it polls a compacted copy of their entries.
            int poll_sockets(const std::span<network::poll_entry> entries) override
            {
                int ready_count = 0;
                std::vector<network::poll_entry> fallback_entries{};
                std::vector<size_t> fallback_indices{};

                for (size_t i = 0; i < entries.size(); ++i)
                {
                    auto& entry = entries[i];
                    auto* socket = dynamic_cast<rust_socket*>(entry.s);
                    if (!socket)
                    {
                        fallback_entries.push_back(entry);
                        fallback_indices.push_back(i);
                        continue;
                    }

                    entry.revents = to_poll_events(socket->readiness(), entry.events);
                    if (entry.revents != 0)
                    {
                        ++ready_count;
                    }
                }

                if (fallback_entries.empty())
                {
                    return ready_count;
                }

                const auto fallback_count = this->fallback_->poll_sockets(fallback_entries);
                for (size_t i = 0; i < fallback_entries.size(); ++i)
                {
                    entries[fallback_indices[i]] = fallback_entries[i];
                }

                return fallback_count < 0 ? fallback_count : ready_count + fallback_count;
            }

          private:
            ::rust::Box<SocketFactory> factory_;
            std::unique_ptr<network::socket_factory> fallback_;
        };

        timespec to_timespec(const int64_t nanoseconds)
        {
            constexpr int64_t nanoseconds_per_second = 1'000'000'000;
            auto seconds = nanoseconds / nanoseconds_per_second;
            auto remainder = nanoseconds % nanoseconds_per_second;
            if (remainder < 0)
            {
                --seconds;
                remainder += nanoseconds_per_second;
            }

            timespec time{};
            time.tv_sec = static_cast<decltype(time.tv_sec)>(seconds);
            time.tv_nsec = static_cast<decltype(time.tv_nsec)>(remainder);
            return time;
        }

        struct compat_stat to_compat_stat(const FileStat& stat)
        {
            struct compat_stat result
            {
            };

            result.st_mode = static_cast<uint32_t>(stat.is_directory ? S_IFDIR : S_IFREG);
            result.st_nlink = 1;
            result.st_size = static_cast<int64_t>(stat.size);
            result.st_atimespec = to_timespec(stat.accessed_nanoseconds);
            result.st_mtimespec = to_timespec(stat.modified_nanoseconds);
            result.st_ctimespec = to_timespec(stat.changed_nanoseconds);
            return result;
        }

        NTSTATUS to_ntstatus(const FileError error)
        {
            switch (error)
            {
            case FileError::Success:
                return STATUS_SUCCESS;
            case FileError::NotFound:
                return STATUS_OBJECT_NAME_NOT_FOUND;
            case FileError::PermissionDenied:
                return STATUS_ACCESS_DENIED;
            case FileError::AlreadyExists:
                return STATUS_OBJECT_NAME_COLLISION;
            case FileError::IsADirectory:
                return STATUS_FILE_IS_A_DIRECTORY;
            case FileError::NotADirectory:
                return STATUS_NOT_A_DIRECTORY;
            case FileError::DirectoryNotEmpty:
                return STATUS_DIRECTORY_NOT_EMPTY;
            default:
                return STATUS_NOT_SUPPORTED;
            }
        }

        OpenMode to_open_mode(const std::u16string_view mode)
        {
            const auto update = mode.find(u'+') != std::u16string_view::npos;
            switch (mode.empty() ? u'\0' : mode.front())
            {
            case u'r':
                return update ? OpenMode::ReadWrite : OpenMode::Read;
            case u'w':
                return update ? OpenMode::WriteRead : OpenMode::Write;
            case u'a':
                return update ? OpenMode::AppendRead : OpenMode::Append;
            default:
                throw std::invalid_argument("Unsupported file open mode: " + u16_to_u8(mode));
            }
        }

        class rust_file : public i_file
        {
          public:
            explicit rust_file(::rust::Box<File> file)
                : file_(std::move(file))
            {
            }

            size_t read(const std::span<std::byte> buffer) override
            {
                return this->file_->read(to_mutable_slice(buffer));
            }

            size_t write(const std::span<const std::byte> buffer) override
            {
                return this->file_->write(to_slice(buffer));
            }

            bool flush() override
            {
                return this->file_->flush();
            }

            bool seek(const int64_t offset, const int origin) override
            {
                switch (origin)
                {
                case SEEK_SET:
                    return this->file_->seek(offset, SeekOrigin::Start);
                case SEEK_CUR:
                    return this->file_->seek(offset, SeekOrigin::Current);
                case SEEK_END:
                    return this->file_->seek(offset, SeekOrigin::End);
                default:
                    return false;
                }
            }

            int64_t tell() override
            {
                return this->file_->tell();
            }

            int64_t size() override
            {
                return this->file_->size();
            }

            bool resize(const uint64_t size) override
            {
                return this->file_->resize(size);
            }

            std::optional<struct compat_stat> stat() override
            {
                FileStat stat{};
                if (!this->file_->stat(stat))
                {
                    return std::nullopt;
                }

                return to_compat_stat(stat);
            }

          private:
            ::rust::Box<File> file_;
        };

        class rust_mount_provider : public mount_provider
        {
          public:
            explicit rust_mount_provider(::rust::Box<FilesystemProvider> provider)
                : provider_(std::move(provider))
            {
            }

            std::optional<struct compat_stat> stat(const windows_path& path) const override
            {
                FileStat stat{};
                if (!this->provider_->file_info(provider_path(path), stat))
                {
                    return std::nullopt;
                }

                return to_compat_stat(stat);
            }

            std::vector<directory_entry> list(const windows_path& path) const override
            {
                const auto entries = this->provider_->read_directory(provider_path(path));
                std::vector<directory_entry> result{};
                result.reserve(entries.size());
                for (const auto& entry : entries)
                {
                    result.push_back(directory_entry{
                        .name = u8_to_u16(to_view(entry.name)),
                        .short_name = u8_to_u16(to_view(entry.short_name)),
                        .is_directory = entry.is_directory,
                        .stat = entry.has_stat ? std::optional(to_compat_stat(entry.stat)) : std::nullopt,
                    });
                }

                return result;
            }

            open_result open(const windows_path& path, const std::u16string& mode) override
            {
                auto error = FileError::Success;
                auto file = this->provider_->open(provider_path(path), to_open_mode(mode), error);
                if (!file->is_open())
                {
                    return {.status = to_ntstatus(error)};
                }

                return {.file = std::make_unique<rust_file>(std::move(file)), .status = STATUS_SUCCESS};
            }

            NTSTATUS create_directory(const windows_path& path) override
            {
                return to_ntstatus(this->provider_->create_directory(provider_path(path)));
            }

            NTSTATUS rename(const windows_path& from, const windows_path& to) override
            {
                return to_ntstatus(this->provider_->rename(provider_path(from), provider_path(to)));
            }

            NTSTATUS remove(const windows_path& path) override
            {
                return to_ntstatus(this->provider_->remove(provider_path(path)));
            }

            std::optional<std::filesystem::path> get_host_path(const windows_path& path) const override
            {
                ::rust::String host_path{};
                if (!this->provider_->host_path(provider_path(path), host_path))
                {
                    return std::nullopt;
                }

                return to_path(host_path);
            }

          private:
            ::rust::Box<FilesystemProvider> provider_;

            static ::rust::String provider_path(const windows_path& path)
            {
                return from_utf16(path.u16string());
            }
        };

        emulator_interfaces make_interfaces(::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                            ::rust::Box<SocketFactory> socket_factory, const network::network_mode mode)
        {
            emulator_interfaces interfaces{};
            if (resolver->is_active())
            {
                interfaces.dns_lookup = std::make_unique<rust_dns_lookup>(std::move(resolver), network::create_dns_lookup(mode));
            }

            if (clock->is_active())
            {
                interfaces.clock = std::make_unique<rust_clock>(std::move(clock));
            }

            if (socket_factory->is_active())
            {
                interfaces.socket_factory =
                    std::make_unique<rust_socket_factory>(std::move(socket_factory), network::create_socket_factory(mode));
            }

            return interfaces;
        }

        std::unique_ptr<guest_machine> create_machine(const machine_type machine, const EmulatorSettings& settings,
                                                      bool& unsupported_vcpu_count)
        {
            const auto backend = settings.has_backend ? std::optional(settings.backend) : std::nullopt;
            if (settings.vcpu_count == 1)
            {
                return create_emulator_for_machine(machine, backend);
            }

            try
            {
                return create_emulator_for_machine(machine, backend, settings.vcpu_count);
            }
            catch (const std::invalid_argument&)
            {
                unsupported_vcpu_count = true;
                return nullptr;
            }
        }

        constexpr std::string_view managed_process_flag = "--sogen-managed-process";

        std::unique_ptr<process_manager> make_process_manager(const EmulatorSettings& settings)
        {
            if (settings.child_executable.empty())
            {
                return nullptr;
            }

            std::vector<std::string> arguments{};
            arguments.reserve(settings.child_arguments.size());
            for (const auto& argument : settings.child_arguments)
            {
                arguments.emplace_back(to_view(argument));
            }

            return std::make_unique<subprocess_process_manager>(
                to_path(settings.child_executable), [arguments = std::move(arguments)](const uint16_t port, const std::string& token) {
                    auto command = arguments;
                    command.emplace_back(managed_process_flag);
                    command.push_back(std::to_string(port));
                    command.push_back(token);
                    return command;
                });
        }

        std::unique_ptr<WindowsEmulator> create_application_emulator(const EmulatorSettings& settings, application_settings application,
                                                                     ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                     ::rust::Box<SocketFactory> socket_factory,
                                                                     bool& unsupported_vcpu_count,
                                                                     std::unique_ptr<ManagedProcess> managed_process)
        {
            const auto native_settings = make_emulator_settings(settings);
            auto machine =
                create_machine(get_machine_type(get_application_machine(native_settings, application)), settings, unsupported_vcpu_count);
            if (!machine)
            {
                return nullptr;
            }

            auto processes = make_process_manager(settings);
            auto interfaces = make_interfaces(std::move(resolver), std::move(clock), std::move(socket_factory), native_settings.network);
            interfaces.processes = processes.get();
            auto emulator = std::make_unique<windows_emulator>(std::move(machine), std::move(application), native_settings,
                                                               emulator_callbacks{}, std::move(interfaces));

            std::unique_ptr<ManagedChild> managed_child{};
            if (managed_process)
            {
                auto& connection = managed_process->connection;
                emulator->setup_process_if_necessary();
                install_inherited_pipes(*emulator, connection.request(), connection.take_inherited_pipes());
                managed_child = std::make_unique<ManagedChild>(ManagedChild{
                    .process = std::move(managed_process),
                    .target = std::make_unique<gated_process_target>(*emulator),
                });
            }

            return std::make_unique<WindowsEmulator>(std::move(processes), std::move(emulator), describe_configuration(native_settings),
                                                     std::move(managed_child));
        }

        vcpu_context& dispatching_vcpu(windows_emulator& emulator)
        {
            return emulator.vcpu(emulator.active_cpu().index());
        }

        vcpu_context* find_running_vcpu(windows_emulator& emulator, const emulator_thread& thread)
        {
            for (uint32_t i = 0; i < emulator.vcpu_count(); ++i)
            {
                if (auto& vcpu = emulator.vcpu(i); vcpu.active_thread == &thread)
                {
                    return &vcpu;
                }
            }

            return nullptr;
        }

        vcpu_context* accessible_running_vcpu(windows_emulator& emulator, const emulator_thread& thread)
        {
            if (!emulator.kernel_lock_held())
            {
                return find_running_vcpu(emulator, thread);
            }

            emulator.wait_for_register_transfer(thread);
            auto* vcpu = find_running_vcpu(emulator, thread);
            if (vcpu && vcpu != &dispatching_vcpu(emulator))
            {
                throw std::runtime_error("The thread is running on another virtual CPU");
            }

            return vcpu;
        }

        void fill_module(const mapped_module& source, ModuleInfo& module)
        {
            module.name = from_utf8(source.name);
            module.path = from_path(source.path);
            module.module_path = from_utf16(source.module_path.u16string());
            module.image_base = source.image_base;
            module.image_base_file = source.image_base_file;
            module.size_of_image = source.size_of_image;
            module.entry_point = source.entry_point;
            module.is_static = source.is_static;

            module.exports.clear();
            module.exports.reserve(source.exports.size());
            for (const auto& symbol : source.exports)
            {
                module.exports.push_back(ExportedSymbol{
                    .name = from_utf8(symbol.name),
                    .ordinal = symbol.ordinal,
                    .rva = symbol.rva,
                    .address = symbol.address,
                });
            }

            module.address_names.clear();
            module.address_names.reserve(source.address_names.size());
            for (const auto& [address, name] : source.address_names)
            {
                module.address_names.push_back(AddressName{.address = address, .name = from_utf8(name)});
            }

            module.machine = source.machine;
            module.characteristics = source.characteristics;
            module.dll_characteristics = source.dll_characteristics;
            module.size_of_stack_reserve = source.size_of_stack_reserve;
            module.size_of_stack_commit = source.size_of_stack_commit;
            module.size_of_heap_reserve = source.size_of_heap_reserve;
            module.size_of_heap_commit = source.size_of_heap_commit;

            module.sections.clear();
            module.sections.reserve(source.sections.size());
            for (const auto& section : source.sections)
            {
                module.sections.push_back(SectionInfo{
                    .name = from_utf8(section.name),
                    .start = section.region.start,
                    .length = static_cast<uint64_t>(section.region.length),
                    .permissions = section.region.permissions,
                    .has_first_execution = section.first_execute.has_value(),
                    .first_execution = section.first_execute.value_or(0),
                });
            }

            module.imported_modules.clear();
            module.imported_modules.reserve(source.imported_modules.size());
            for (const auto& name : source.imported_modules)
            {
                module.imported_modules.push_back(from_utf8(name));
            }

            module.imports.clear();
            module.imports.reserve(source.imports.size());
            for (const auto& [slot, symbol] : source.imports)
            {
                module.imports.push_back(ImportInfo{
                    .module = symbol.module_index < source.imported_modules.size() ? from_utf8(source.imported_modules[symbol.module_index])
                                                                                   : ::rust::String{},
                    .name = from_utf8(symbol.name),
                    .slot = slot,
                });
            }
        }

        void fill_thread(const emulator_thread& source, utils::clock& clock, ThreadInfo& thread)
        {
            thread.id = source.id;
            thread.name = from_utf16(source.name);
            thread.start_address = source.start_address;
            thread.argument = source.argument;
            thread.executed_instructions = source.executed_instructions;
            thread.executed_blocks = source.executed_blocks;
            thread.current_ip = source.current_ip;
            thread.previous_ip = source.previous_ip;
            thread.setup_done = source.setup_done;
            thread.has_exit_status = source.exit_status.has_value();
            thread.exit_status = static_cast<uint32_t>(source.exit_status.value_or(0));
            thread.stack_base = source.stack_base;
            thread.stack_size = source.stack_size;
            thread.has_wow64_stack = source.wow64_stack_base.has_value();
            thread.wow64_stack_base = source.wow64_stack_base.value_or(0);
            thread.wow64_stack_size = source.wow64_stack_size.value_or(0);
            thread.has_teb = source.teb64.has_value();
            thread.teb_address = source.teb64 ? source.teb64->value() : 0;
            thread.has_compatibility_teb = source.teb32.has_value();
            thread.compatibility_teb_address = source.teb32 ? source.teb32->value() : 0;
            thread.suspended = source.suspended;
            thread.debugger_hide = source.debugger_hide;
            thread.create_flags = source.create_flags;

            thread.waiting = !source.await_objects.empty() || source.await_time.has_value() || source.await_msg.has_value() ||
                             source.await_io_completion.has_value() || source.await_file_io.has_value() || source.waiting_for_alert ||
                             static_cast<bool>(source.await_host_condition);
            thread.waited_handles.clear();
            thread.waited_handles.reserve(source.await_objects.size());
            for (const auto object : source.await_objects)
            {
                thread.waited_handles.push_back(object.bits);
            }

            thread.wait_any = source.await_any;
            thread.has_timeout = source.await_time.has_value() && *source.await_time != std::chrono::steady_clock::time_point::min();
            thread.timeout_nanoseconds = thread.has_timeout ? remaining_nanoseconds(*source.await_time, clock.steady_now()) : 0;
            thread.alertable = source.apc_alertable;
            thread.pending_apc_count = static_cast<uint64_t>(source.pending_apcs.size());
        }

        RegionInfo make_region(const region_info& info)
        {
            return RegionInfo{
                .start = info.start,
                .length = static_cast<uint64_t>(info.length),
                .permissions = info.permissions.flattened(),
                .allocation_base = info.allocation_base,
                .allocation_length = static_cast<uint64_t>(info.allocation_length),
                .is_reserved = info.is_reserved,
                .is_committed = info.is_committed,
                .initial_permissions = info.initial_permissions.flattened(),
                .kind = info.kind,
            };
        }

        HandleRecord make_handle_record(const handle value, const ref_counted_object& object, utils::clock& clock)
        {
            HandleRecord record{};
            record.value = value.bits;
            record.kind = static_cast<HandleType>(value.value.type);

            switch (value.value.type)
            {
            case handle_types::file: {
                const auto& entry = static_cast<const file&>(object);
                record.path = from_utf16(entry.name);
                record.is_directory = entry.is_directory();
                break;
            }
            case handle_types::event: {
                const auto& entry = static_cast<const event&>(object);
                record.name = from_utf16(entry.name);
                record.signalled = entry.signaled;
                record.event_type = entry.type;
                break;
            }
            case handle_types::section: {
                const auto& entry = static_cast<const section&>(object);
                record.name = from_utf16(entry.name);
                record.path = from_utf16(entry.file_name);
                record.maximum_size = entry.maximum_size;
                record.page_protection = entry.section_page_protection;
                record.allocation_attributes = entry.allocation_attributes;
                break;
            }
            case handle_types::semaphore: {
                const auto& entry = static_cast<const semaphore&>(object);
                record.name = from_utf16(entry.name);
                record.current_count = entry.current_count;
                record.maximum_count = entry.max_count;
                break;
            }
            case handle_types::mutant: {
                const auto& entry = static_cast<const mutant&>(object);
                record.name = from_utf16(entry.name);
                record.id = entry.owning_thread_id;
                record.current_count = entry.locked_count;
                record.abandoned = entry.abandoned;
                break;
            }
            case handle_types::timer: {
                const auto& entry = static_cast<const timer&>(object);
                record.name = from_utf16(entry.name);
                record.signalled = entry.signaled;
                record.has_due_time = entry.due_time.has_value();
                record.due_in_nanoseconds = entry.due_time ? remaining_nanoseconds(*entry.due_time, clock.steady_now()) : 0;
                record.period_nanoseconds = static_cast<uint64_t>(std::max<int64_t>(entry.period_nanoseconds, 0));
                break;
            }
            case handle_types::job: {
                const auto& entry = static_cast<const job_object&>(object);
                record.name = from_utf16(entry.name);
                record.limit_flags = entry.limit_flags;
                break;
            }
            case handle_types::registry:
                record.path = from_utf16(static_cast<const registry_key&>(object).to_string());
                break;
            case handle_types::port:
                record.name = from_utf16(static_cast<const port_container&>(object).get_port_name());
                break;
            case handle_types::process:
                record.id = static_cast<const emulator_process&>(object).id;
                break;
            case handle_types::thread:
                record.id = static_cast<const emulator_thread&>(object).id;
                break;
            default:
                break;
            }

            return record;
        }

        void fill_mapped_module(const mapped_module* module, ModuleInfo& info)
        {
            if (!module)
            {
                throw std::runtime_error("The module could not be mapped");
            }

            fill_module(*module, info);
        }

        std::span<const std::byte> as_span(const std::string& text)
        {
            return as_bytes(std::string_view(text));
        }
    }

    gated_process_target::gated_process_target(windows_emulator& emulator)
        : target_(emulator)
    {
    }

    void gated_process_target::open()
    {
        {
            const std::scoped_lock lock(this->mutex_);
            this->open_ = true;
        }

        this->opened_.notify_all();
    }

    void gated_process_target::close()
    {
        const std::scoped_lock lock(this->mutex_);
        this->open_ = false;
    }

    uint64_t gated_process_target::native_environment() const
    {
        return this->when_open([&] { return this->target_.native_environment(); });
    }

    uint64_t gated_process_target::compatibility_environment() const
    {
        return this->when_open([&] { return this->target_.compatibility_environment(); });
    }

    uint64_t gated_process_target::native_parameters() const
    {
        return this->when_open([&] { return this->target_.native_parameters(); });
    }

    uint64_t gated_process_target::compatibility_parameters() const
    {
        return this->when_open([&] { return this->target_.compatibility_parameters(); });
    }

    process_memory_result gated_process_target::allocate_memory(const uint64_t address, const uint64_t size,
                                                                const memory_permission permission, const bool reserve, const bool commit)
    {
        return this->when_open([&] { return this->target_.allocate_memory(address, size, permission, reserve, commit); });
    }

    process_error gated_process_target::free_memory(const uint64_t address, const uint64_t size, const bool release)
    {
        return this->when_open([&] { return this->target_.free_memory(address, size, release); });
    }

    process_memory_result gated_process_target::protect_memory(const uint64_t address, const uint64_t size,
                                                               const memory_permission permission)
    {
        return this->when_open([&] { return this->target_.protect_memory(address, size, permission); });
    }

    process_memory_read_result gated_process_target::read_memory(const uint64_t address, const uint64_t size) const
    {
        return this->when_open([&] { return this->target_.read_memory(address, size); });
    }

    process_memory_result gated_process_target::write_memory(const uint64_t address, const std::span<const uint8_t> data)
    {
        return this->when_open([&] { return this->target_.write_memory(address, data); });
    }

    ApplicationSettings ManagedProcess::application() const
    {
        const auto& request = this->connection.request();
        ApplicationSettings application{
            .application = ::rust::String(request.application),
            .argument0 = ::rust::String(request.argument0),
            .working_directory = ::rust::String(request.working_directory),
            .has_command_line = true,
            .command_line = ::rust::String(request.command_line),
            .has_process_id = true,
            .process_id = request.process_id,
            .has_thread_id = true,
            .thread_id = request.thread_id,
        };

        for (const auto& argument : request.arguments)
        {
            application.arguments.emplace_back(argument);
        }

        for (const auto& [name, value] : request.environment)
        {
            application.environment.push_back(EnvironmentVariable{.name = ::rust::String(name), .value = ::rust::String(value)});
        }

        return application;
    }

    WindowsEmulator::WindowsEmulator(std::unique_ptr<process_manager> processes, std::unique_ptr<windows_emulator> emulator,
                                     std::string configuration, std::unique_ptr<ManagedChild> managed_child)
        : processes_(std::move(processes)),
          emulator_(std::move(emulator)),
          machine_(
              this->emulator_->emu(), [this]() -> guest_cpu& { return this->emulator_->active_cpu(); },
              [emulator = this->emulator_.get()](cpu_interface& cpu, const std::function<void()>& callback) {
                  if (!emulator->kernel_lock_held())
                  {
                      emulator->dispatch_on_cpu(cpu, callback);
                      return;
                  }

                  const windows_emulator::scoped_dispatch dispatch(*emulator, emulator->vcpu(cpu.index()));
                  callback();
              }),
          configuration_(std::move(configuration)),
          managed_child_(std::move(managed_child))
    {
        this->install_callbacks();
    }

    WindowsEmulator::~WindowsEmulator()
    {
        for (auto& callback : this->callbacks_)
        {
            callback.reset();
        }

        this->module_load_observer_.reset();
        this->module_unload_observer_.reset();

        if (this->managed_child_)
        {
            this->managed_child_->target->open();
            this->managed_child_->process->connection.disconnect();
        }
    }

    void WindowsEmulator::install_callbacks()
    {
        auto& callbacks = this->emulator_->callbacks;

        callbacks.on_module_load.add([this](const mapped_module& module) {
            this->emit(Event::ModuleLoad, module.image_base);
            if (const auto observer = this->module_load_observer_)
            {
                this->run_serialised([&] { (*observer)(module.image_base); });
            }
        });
        callbacks.on_module_unload.add([this](const mapped_module& module) {
            this->emit(Event::ModuleUnload, module.image_base);
            if (const auto observer = this->module_unload_observer_)
            {
                this->run_serialised([&] { (*observer)(module.image_base); });
            }
        });
        callbacks.on_stdout = [this](const std::string_view data) { this->emit(Event::Stdout, 0, 0, 0, 0, as_bytes(data)); };
        callbacks.on_syscall = [this](const uint32_t syscall_id, const std::string_view syscall_name) {
            return static_cast<instruction_hook_continuation>(this->emit(Event::Syscall, syscall_id, 0, 0, 0, as_bytes(syscall_name)));
        };
        callbacks.on_generic_access = [this](const std::string_view type, const std::u16string_view name) {
            this->emit(Event::GenericAccess, 0, 0, 0, 0, as_bytes(type), as_span(u16_to_u8(name)));
        };
        callbacks.on_generic_activity = [this](const std::string_view description) {
            this->emit(Event::GenericActivity, 0, 0, 0, 0, as_bytes(description));
        };
        callbacks.on_suspicious_activity = [this](const std::string_view description) {
            this->emit(Event::SuspiciousActivity, 0, 0, 0, 0, as_bytes(description));
        };
        callbacks.on_socket_activity = [this](const network::socket_activity activity, const network::address& address) {
            std::array<std::byte, 16> octets{};
            size_t length{};
            uint64_t scope_id{};
            if (address.is_ipv4())
            {
                length = sizeof(in_addr);
                std::memcpy(octets.data(), &address.get_in_addr().sin_addr, length);
            }
            else if (address.is_ipv6())
            {
                length = sizeof(in6_addr);
                std::memcpy(octets.data(), &address.get_in6_addr().sin6_addr, length);
                scope_id = address.get_in6_addr().sin6_scope_id;
            }
            else
            {
                throw std::runtime_error("Unsupported socket address family: " + std::to_string(address.get_family()));
            }

            this->emit(Event::SocketActivity, static_cast<uint64_t>(activity), address.get_port(), scope_id, 0,
                       std::span<const std::byte>(octets.data(), length));
        };
        callbacks.on_exception = [this] { this->emit(Event::Exception); };
        callbacks.on_instruction = [this](const uint64_t address) { this->emit(Event::Instruction, address); };
        callbacks.on_memory_protect = [this](const uint64_t address, const uint64_t length, const memory_permission permission) {
            this->emit(Event::MemoryProtect, address, length, static_cast<uint64_t>(permission));
        };
        callbacks.on_memory_allocate = [this](const uint64_t address, const uint64_t length, const memory_permission permission,
                                              const bool commit) {
            this->emit(Event::MemoryAllocate, address, length, static_cast<uint64_t>(permission), commit ? 1 : 0);
        };
        callbacks.on_memory_violate = [this](const uint64_t address, const uint64_t length, const memory_operation operation,
                                             const memory_violation_type type) {
            this->emit(Event::MemoryViolate, address, length, static_cast<uint64_t>(operation), static_cast<uint64_t>(type));
        };
        callbacks.on_rdtsc = [this] { this->emit(Event::Rdtsc); };
        callbacks.on_rdtscp = [this] { this->emit(Event::Rdtscp); };
        callbacks.on_ioctrl = [this](io_device&, const std::u16string_view device_name, const ULONG code) {
            this->emit(Event::Ioctrl, static_cast<uint32_t>(code), 0, 0, 0, as_span(u16_to_u8(device_name)));
        };
        callbacks.on_debug_string.add(
            [this](const std::string_view message) { this->emit(Event::DebugString, 0, 0, 0, 0, as_bytes(message)); });
        callbacks.on_thread_create = [this](const handle thread_handle, const emulator_thread& thread) {
            this->emit(Event::ThreadCreate, thread_handle.bits, thread.id, thread.start_address, thread.argument);
        };
        callbacks.on_thread_terminated = [this](const handle thread_handle, const emulator_thread& thread) {
            this->emit(Event::ThreadTerminated, thread_handle.bits, thread.id);
        };
        callbacks.on_thread_set_name = [this](const emulator_thread& thread) {
            this->emit(Event::ThreadSetName, thread.id, 0, 0, 0, as_span(u16_to_u8(thread.name)));
        };
        callbacks.on_thread_switch = [this](const emulator_thread& current_thread, const emulator_thread& new_thread) {
            this->emit(Event::ThreadSwitch, current_thread.id, new_thread.id);
        };
        callbacks.on_fast_fail = [this](const uint32_t code) { this->emit(Event::FastFail, code); };
        callbacks.on_section_first_execution.add(
            [this](const mapped_module& module, const mapped_section& section, const uint64_t address) {
                const auto index = static_cast<uint64_t>(&section - module.sections.data());
                this->emit(Event::SectionFirstExecution, module.image_base, index, address);
            });
        this->emulator_->log.set_sink([this](const color c, const std::string_view message) {
            this->emit(Event::Log, static_cast<uint64_t>(c), 0, 0, 0, as_bytes(message));
        });
    }

    uint64_t WindowsEmulator::emit(const Event event, const uint64_t a, const uint64_t b, const uint64_t c, const uint64_t d,
                                   const std::span<const std::byte> first, const std::span<const std::byte> second) const
    {
        return this->run_serialised([&]() -> uint64_t {
            // A copy, so a callback that replaces or clears its own slot keeps running on a live closure.
            const auto callback = this->callbacks_.at(static_cast<size_t>(event));
            return callback ? (*callback)(a, b, c, d, first, second) : 0;
        });
    }

    void WindowsEmulator::set_callback(const Event event, std::unique_ptr<CallbackHandle> callback) const
    {
        this->callbacks_.at(static_cast<size_t>(event)) = std::move(callback);
    }

    void WindowsEmulator::observe_modules(std::unique_ptr<CallbackHandle> load, std::unique_ptr<CallbackHandle> unload) const
    {
        this->module_load_observer_ = std::move(load);
        this->module_unload_observer_ = std::move(unload);
    }

    const Machine& WindowsEmulator::machine() const
    {
        return this->machine_;
    }

    void WindowsEmulator::start(const uint64_t count) const
    {
        auto* const managed_child = this->managed_child_.get();
        if (!managed_child)
        {
            this->emulator_->start(static_cast<size_t>(count));
            return;
        }

        managed_child->target->open();
        const auto _ = utils::finally([managed_child] { managed_child->target->close(); });

        if (managed_child->awaiting_resume)
        {
            if (!managed_child->process->connection.wait_for_resume(*managed_child->target))
            {
                throw std::runtime_error("Acknowledging managed process startup failed");
            }

            managed_child->awaiting_resume = false;
        }

        this->emulator_->start(static_cast<size_t>(count));

        const auto& exit_status = this->emulator_->process.exit_status;
        if (!managed_child->exit_reported && exit_status.has_value())
        {
            managed_child->exit_reported = true;
            if (!managed_child->process->connection.notify_exit(static_cast<uint32_t>(*exit_status)))
            {
                throw std::runtime_error("Reporting managed process exit failed");
            }
        }
    }

    void WindowsEmulator::stop() const
    {
        this->emulator_->stop();
    }

    void WindowsEmulator::setup_process_if_necessary() const
    {
        this->emulator_->setup_process_if_necessary();
    }

    void WindowsEmulator::yield_thread(const bool alertable) const
    {
        auto& emulator = *this->emulator_;
        emulator.synchronize([&] { emulator.yield_thread(emulator.vcpu(0), alertable); });
    }

    bool WindowsEmulator::perform_thread_switch() const
    {
        return this->emulator_->perform_thread_switch(this->emulator_->vcpu(0));
    }

    bool WindowsEmulator::activate_thread(const uint32_t id) const
    {
        return this->emulator_->activate_thread(this->emulator_->vcpu(0), id);
    }

    void WindowsEmulator::save_snapshot() const
    {
        this->emulator_->save_snapshot();
    }

    void WindowsEmulator::restore_snapshot() const
    {
        this->emulator_->restore_snapshot();
    }

    ::rust::Vec<uint8_t> WindowsEmulator::serialize_state() const
    {
        utils::buffer_serializer serializer{};
        this->emulator_->serialize(serializer);
        return to_rust_bytes(serializer.move_buffer());
    }

    void WindowsEmulator::deserialize_state(const ::rust::Slice<const uint8_t> data) const
    {
        utils::buffer_deserializer deserializer{std::as_bytes(std::span(data.data(), data.size()))};
        this->emulator_->deserialize(deserializer);
    }

    void WindowsEmulator::write_snapshot(const ::rust::Str path) const
    {
        snapshot::write_emulator_snapshot(*this->emulator_, to_path(path), this->configuration_);
    }

    uint64_t WindowsEmulator::executed_instructions() const
    {
        return this->emulator_->get_executed_instructions();
    }

    stop_reason WindowsEmulator::last_stop_reason() const
    {
        return this->emulator_->last_stop_reason();
    }

    ::rust::String WindowsEmulator::last_stop_detail() const
    {
        return from_utf8(this->emulator_->last_stop_detail());
    }

    ::rust::String WindowsEmulator::emulation_root() const
    {
        return from_path(this->emulator_->emulation_root);
    }

    uint16_t WindowsEmulator::host_port(const uint16_t emulator_port) const
    {
        return this->emulator_->get_host_port(emulator_port);
    }

    uint16_t WindowsEmulator::emulator_port(const uint16_t host_port) const
    {
        return this->emulator_->get_emulator_port(host_port);
    }

    void WindowsEmulator::map_port(const uint16_t emulator_port, const uint16_t host_port) const
    {
        this->emulator_->map_port(emulator_port, host_port);
    }

    ProcessInfo WindowsEmulator::process_info() const
    {
        const auto& process = this->emulator_->process;
        return ProcessInfo{
            .is_wow64 = process.is_wow64_process,
            .has_exit_status = process.exit_status.has_value(),
            .exit_status = static_cast<uint32_t>(process.exit_status.value_or(0)),
            .live_thread_count = static_cast<uint64_t>(process.get_live_thread_count()),
            .spawned_thread_count = process.spawned_thread_count,
            .process_id = process.process_id,
            .initial_thread_id = process.initial_thread_id,
            .peb_address = process.peb64.value(),
            .has_compatibility_peb = process.peb32.has_value(),
            .compatibility_peb_address = process.peb32 ? process.peb32->value() : 0,
            .process_parameters_address = process.process_params64.value(),
            .hard_error_mode = process.hard_error_mode,
        };
    }

    bool WindowsEmulator::active_thread(ThreadInfo& thread) const
    {
        const auto* active = dispatching_vcpu(*this->emulator_).active_thread;
        if (!active)
        {
            return false;
        }

        fill_thread(*active, this->emulator_->clock(), thread);
        return true;
    }

    bool WindowsEmulator::current_thread_id(uint32_t& id) const
    {
        const auto* active = dispatching_vcpu(*this->emulator_).active_thread;
        if (!active)
        {
            return false;
        }

        id = active->id;
        return true;
    }

    uint32_t WindowsEmulator::vcpu_count() const
    {
        return this->emulator_->vcpu_count();
    }

    uint32_t WindowsEmulator::current_vcpu() const
    {
        return static_cast<uint32_t>(this->emulator_->active_cpu().index());
    }

    ::rust::Vec<ThreadInfo> WindowsEmulator::threads() const
    {
        auto& emulator = *this->emulator_;
        ::rust::Vec<ThreadInfo> result{};
        result.reserve(emulator.process.threads.size());
        for (const auto& thread : emulator.process.threads | std::views::values)
        {
            ThreadInfo info{};
            fill_thread(thread, emulator.clock(), info);
            result.push_back(std::move(info));
        }

        return result;
    }

    bool WindowsEmulator::thread(const uint32_t id, ThreadInfo& thread) const
    {
        const auto* found = this->emulator_->process.find_thread_by_id(id);
        if (!found)
        {
            return false;
        }

        fill_thread(*found, this->emulator_->clock(), thread);
        return true;
    }

    bool WindowsEmulator::thread_registers(const uint32_t id, ::rust::Vec<uint8_t>& registers) const
    {
        auto& emulator = *this->emulator_;
        const auto* found = emulator.process.find_thread_by_id(id);
        if (!found)
        {
            return false;
        }

        const auto* vcpu = accessible_running_vcpu(emulator, *found);
        registers = vcpu ? to_rust_bytes(vcpu->cpu.save_registers()) : to_rust_bytes(found->last_registers);
        return true;
    }

    bool WindowsEmulator::set_thread_registers(const uint32_t id, const ::rust::Slice<const uint8_t> registers) const
    {
        auto& emulator = *this->emulator_;
        auto* found = emulator.process.find_thread_by_id(id);
        if (!found)
        {
            return false;
        }

        if (const auto* vcpu = accessible_running_vcpu(emulator, *found))
        {
            vcpu->cpu.restore_registers(to_bytes(registers));
            return true;
        }

        if (registers.size() != found->last_registers.size())
        {
            throw std::invalid_argument("The register state does not match the guest CPU");
        }

        found->last_registers = to_bytes(registers);
        return true;
    }

    uint32_t WindowsEmulator::create_thread(const uint64_t start_address, const uint64_t argument, const bool suspended) const
    {
        auto& emulator = *this->emulator_;
        const auto* executable = emulator.mod_manager.executable;
        auto stack_size = executable ? executable->size_of_stack_reserve : 0;
        if (stack_size == 0)
        {
            stack_size = STACK_SIZE;
        }

        const auto thread_handle =
            emulator.process.create_thread(emulator.memory, start_address, argument, align_up(stack_size, ALLOCATION_GRANULARITY),
                                           suspended ? THREAD_CREATE_FLAGS_CREATE_SUSPENDED : 0);
        if (!suspended)
        {
            emulator.wake_idle_vcpu();
        }

        return emulator.process.threads.get(thread_handle)->id;
    }

    bool WindowsEmulator::terminate_thread(const uint32_t id, const uint32_t exit_status) const
    {
        auto& emulator = *this->emulator_;
        const auto terminate = [&] {
            auto& process = emulator.process;
            auto* found = process.find_thread_by_id(id);
            if (!found)
            {
                return false;
            }

            process.terminate_thread(*found, static_cast<NTSTATUS>(exit_status));
            emulator.callbacks.on_thread_terminated(process.threads.find_handle(found), *found);
            emulator.wake_idle_vcpu();

            if (auto* vcpu = find_running_vcpu(emulator, *found))
            {
                emulator.yield_thread(*vcpu);
            }

            return true;
        };

        return this->run_serialised(terminate);
    }

    ::rust::Vec<HandleRecord> WindowsEmulator::handles() const
    {
        auto& emulator = *this->emulator_;
        auto& process = emulator.process;
        ::rust::Vec<HandleRecord> result{};
        const auto collect = [&](const auto& store) {
            for (const auto& [index, object] : store)
            {
                result.push_back(make_handle_record(store.make_handle(index), object, emulator.clock()));
            }
        };

        collect(process.processes);
        collect(process.managed_threads);
        collect(process.threads);
        collect(process.events);
        collect(process.files);
        collect(process.devices);
        collect(process.semaphores);
        collect(process.io_completions);
        collect(process.wait_completion_packets);
        collect(process.worker_factories);
        collect(process.jobs);
        collect(process.registry_keys);
        collect(process.mutants);
        collect(process.timers);
        collect(process.desktops);
        collect(process.ports);
        collect(process.sections);
        collect(process.private_namespaces);
        collect(process.deferred_window_position_batches);
        return result;
    }

    bool WindowsEmulator::handle_info(const uint64_t value, HandleRecord& handle) const
    {
        auto& emulator = *this->emulator_;
        const auto native = make_handle(value);
        auto* store = emulator.process.get_handle_store(native);
        const auto* object = store ? store->get_object(native) : nullptr;
        if (!object)
        {
            return false;
        }

        handle = make_handle_record(native, *object, emulator.clock());
        return true;
    }

    bool WindowsEmulator::atom_name(const uint16_t id, ::rust::String& name) const
    {
        const auto found = this->emulator_->process.get_atom_name(id);
        if (!found)
        {
            return false;
        }

        name = from_utf16(*found);
        return true;
    }

    bool WindowsEmulator::find_atom(const ::rust::Str name, uint16_t& id) const
    {
        const auto wanted = u8_to_u16(to_view(name));
        for (const auto& [atom, entry] : this->emulator_->process.atoms)
        {
            if (utils::string::equals_ignore_case(std::u16string_view{entry.name}, std::u16string_view{wanted}))
            {
                id = atom;
                return true;
            }
        }

        return false;
    }

    uint16_t WindowsEmulator::add_atom(const ::rust::Str name) const
    {
        return this->emulator_->process.add_or_find_atom(u8_to_u16(to_view(name)));
    }

    bool WindowsEmulator::delete_atom(const uint16_t id) const
    {
        return this->emulator_->process.delete_atom(id);
    }

    uint64_t WindowsEmulator::allocate_memory(const uint64_t size, const memory_permission permissions, const bool reserve_only,
                                              const uint64_t start, const memory_region_kind kind, const uint64_t highest_address) const
    {
        return this->emulator_->memory.allocate_memory(static_cast<size_t>(size), nt_memory_permission{permissions}, reserve_only, start,
                                                       kind, highest_address ? highest_address : MAX_ALLOCATION_END_EXCL - 1);
    }

    bool WindowsEmulator::allocate_memory_at(const uint64_t address, const uint64_t size, const memory_permission permissions,
                                             const bool reserve_only, const memory_region_kind kind) const
    {
        return this->emulator_->memory.allocate_memory(address, static_cast<size_t>(size), nt_memory_permission{permissions}, reserve_only,
                                                       kind);
    }

    bool WindowsEmulator::protect_memory(const uint64_t address, const uint64_t size, const memory_permission permissions) const
    {
        return this->emulator_->memory.protect_memory(address, static_cast<size_t>(size), nt_memory_permission{permissions});
    }

    bool WindowsEmulator::commit_memory(const uint64_t address, const uint64_t size, const memory_permission permissions) const
    {
        return this->emulator_->memory.commit_memory(address, static_cast<size_t>(size), nt_memory_permission{permissions});
    }

    bool WindowsEmulator::decommit_memory(const uint64_t address, const uint64_t size) const
    {
        return this->emulator_->memory.decommit_memory(address, static_cast<size_t>(size));
    }

    bool WindowsEmulator::release_memory(const uint64_t address, const uint64_t size) const
    {
        return this->emulator_->memory.release_memory(address, static_cast<size_t>(size));
    }

    uint64_t WindowsEmulator::find_free_allocation_base(const uint64_t size, const uint64_t start) const
    {
        return this->emulator_->memory.find_free_allocation_base(static_cast<size_t>(size), start);
    }

    bool WindowsEmulator::region_info(const uint64_t address, RegionInfo& region) const
    {
        if (address < MIN_ALLOCATION_ADDRESS || address >= MAX_ALLOCATION_END_EXCL)
        {
            return false;
        }

        region = make_region(this->emulator_->memory.get_region_info(address));
        return true;
    }

    MemoryStats WindowsEmulator::memory_stats() const
    {
        const auto stats = this->emulator_->memory.compute_memory_stats();
        return MemoryStats{.reserved_memory = stats.reserved_memory, .committed_memory = stats.committed_memory};
    }

    uint64_t WindowsEmulator::default_allocation_address() const
    {
        return this->emulator_->memory.get_default_allocation_address();
    }

    void WindowsEmulator::set_default_allocation_address(const uint64_t address) const
    {
        this->emulator_->memory.set_default_allocation_address(address);
    }

    ::rust::Vec<RegionInfo> WindowsEmulator::memory_regions() const
    {
        auto& memory = this->emulator_->memory;
        ::rust::Vec<RegionInfo> result{};
        for (const auto& [base, reserved] : memory.get_reserved_regions())
        {
            const auto end = base + reserved.length;
            for (auto address = base; address < end;)
            {
                const auto info = memory.get_region_info(address);
                result.push_back(make_region(info));
                address = info.start + info.length;
            }
        }

        return result;
    }

    uint64_t WindowsEmulator::maximum_allocation_address() const
    {
        return this->emulator_->memory.get_maximum_allocation_address();
    }

    bool WindowsEmulator::dep_enabled() const
    {
        return this->emulator_->memory.is_dep_enabled();
    }

    void WindowsEmulator::set_dep_enabled(const bool enabled) const
    {
        this->emulator_->memory.set_dep_enabled(enabled);
    }

    bool WindowsEmulator::mapped_filename(const uint64_t address, ::rust::String& filename) const
    {
        const auto found = this->emulator_->memory.get_region_mapped_filename(address);
        if (!found)
        {
            return false;
        }

        filename = from_utf16(*found);
        return true;
    }

    bool WindowsEmulator::map_mmio(const uint64_t address, const uint64_t size, ::rust::Box<MmioDevice> device) const
    {
        const std::shared_ptr<const ::rust::Box<MmioDevice>> shared = std::make_shared<::rust::Box<MmioDevice>>(std::move(device));
        return this->emulator_->memory.allocate_mmio(
            address, static_cast<size_t>(size),
            [shared](const uint64_t offset, void* data, const size_t length) {
                (*shared)->read(offset, ::rust::Slice<uint8_t>(static_cast<uint8_t*>(data), length));
            },
            [shared](const uint64_t offset, const void* data, const size_t length) {
                (*shared)->write(offset, ::rust::Slice<const uint8_t>(static_cast<const uint8_t*>(data), length));
            });
    }

    bool WindowsEmulator::map_host_memory_at(const uint64_t address, const uint64_t size, uint8_t* host_memory,
                                             const memory_permission permissions) const
    {
        return this->emulator_->memory.allocate_host_memory_at(address, static_cast<size_t>(size), host_memory,
                                                               nt_memory_permission{permissions});
    }

    uint64_t WindowsEmulator::map_host_memory(const uint64_t size, uint8_t* host_memory, const memory_permission permissions) const
    {
        return this->emulator_->memory.allocate_host_memory(static_cast<size_t>(size), host_memory, nt_memory_permission{permissions});
    }

    RegistryLookup WindowsEmulator::registry_value(const ::rust::Str key, const ::rust::Str name, RegistryData& value) const
    {
        auto& registry = this->emulator_->registry;
        const auto found_key = registry.get_key(utils::path_key{to_path(key)});
        if (!found_key)
        {
            return RegistryLookup::MissingKey;
        }

        const auto found = registry.get_value(*found_key, to_view(name));
        if (!found)
        {
            return RegistryLookup::MissingValue;
        }

        value = RegistryData{
            .name = from_utf8(found->name),
            .kind = found->type,
            .data = to_rust_bytes(found->data),
        };
        return RegistryLookup::Found;
    }

    bool WindowsEmulator::registry_values(const ::rust::Str key, ::rust::Vec<RegistryData>& values) const
    {
        auto& registry = this->emulator_->registry;
        const auto found_key = registry.get_key(utils::path_key{to_path(key)});
        if (!found_key)
        {
            return false;
        }

        const auto count = registry.get_value_count(*found_key);
        values.reserve(count);
        for (size_t index = 0; index < count; ++index)
        {
            if (const auto value = registry.get_value(*found_key, index))
            {
                values.push_back(RegistryData{
                    .name = from_utf8(value->name),
                    .kind = value->type,
                    .data = to_rust_bytes(value->data),
                });
            }
        }

        return true;
    }

    bool WindowsEmulator::registry_subkeys(const ::rust::Str key, ::rust::Vec<::rust::String>& names) const
    {
        auto& registry = this->emulator_->registry;
        const auto found_key = registry.get_key(utils::path_key{to_path(key)});
        if (!found_key)
        {
            return false;
        }

        const auto count = registry.get_sub_key_count(*found_key);
        names.reserve(count);
        for (size_t index = 0; index < count; ++index)
        {
            if (const auto name = registry.get_sub_key_name(*found_key, index))
            {
                names.push_back(from_utf8(*name));
            }
        }

        return true;
    }

    bool WindowsEmulator::create_registry_key(const ::rust::Str key) const
    {
        return this->emulator_->registry.create_key(to_path(key)).has_value();
    }

    bool WindowsEmulator::set_registry_value(const ::rust::Str key, const ::rust::Str name, const uint32_t kind,
                                             const ::rust::Slice<const uint8_t> data) const
    {
        auto& registry = this->emulator_->registry;
        const auto found_key = registry.get_key(utils::path_key{to_path(key)});
        if (!found_key)
        {
            return false;
        }

        registry.set_value(*found_key, std::string{to_view(name)}, kind, std::as_bytes(std::span(data.data(), data.size())));
        return true;
    }

    void WindowsEmulator::import_registry_file(const ::rust::Str path) const
    {
        sogen::import_registry_file(this->emulator_->registry, to_path(path));
    }

    void WindowsEmulator::import_registry(const ::rust::Slice<const uint8_t> contents) const
    {
        import_registry_file_contents(this->emulator_->registry, std::as_bytes(std::span(contents.data(), contents.size())));
    }

    ::rust::String WindowsEmulator::translate_path(const ::rust::Str path) const
    {
        return from_path(this->emulator_->file_sys.translate(to_windows_path(path)));
    }

    ::rust::Vec<uint8_t> WindowsEmulator::drives() const
    {
        ::rust::Vec<uint8_t> result{};
        for (const auto drive : this->emulator_->file_sys.list_drives())
        {
            result.push_back(static_cast<uint8_t>(drive));
        }

        return result;
    }

    bool WindowsEmulator::is_path_writable(const ::rust::Str path) const
    {
        return this->emulator_->file_sys.is_writable(to_windows_path(path));
    }

    bool WindowsEmulator::file_info(const ::rust::Str path, FileStat& stat) const
    {
        const auto found = this->emulator_->file_sys.stat(to_windows_path(path));
        if (!found)
        {
            return false;
        }

        stat = make_file_stat(*found);
        return true;
    }

    ::rust::Vec<DirectoryEntryInfo> WindowsEmulator::read_directory(const ::rust::Str path) const
    {
        const auto entries = this->emulator_->file_sys.list_directory(to_windows_path(path));
        ::rust::Vec<DirectoryEntryInfo> result{};
        result.reserve(entries.size());
        for (const auto& entry : entries)
        {
            result.push_back(DirectoryEntryInfo{
                .name = from_utf16(entry.name),
                .short_name = from_utf16(entry.short_name),
                .is_directory = entry.is_directory,
                .has_stat = entry.stat.has_value(),
                .stat = entry.stat ? make_file_stat(*entry.stat) : FileStat{},
            });
        }

        return result;
    }

    void WindowsEmulator::add_mount(const MountSpec& mount) const
    {
        const auto spec = make_mount_spec(mount);
        this->emulator_->file_sys.map(spec.guest_path, spec.host_path, spec.mode);
        this->configuration_ += describe_mount(spec);
    }

    void WindowsEmulator::mount_provider(const ::rust::Str guest_path, const filesystem_mode mode,
                                         ::rust::Box<FilesystemProvider> provider) const
    {
        this->emulator_->file_sys.mount(to_windows_path(guest_path), std::make_unique<rust_mount_provider>(std::move(provider)), mode);
    }

    WindowsVersionInfo WindowsEmulator::windows_version() const
    {
        const auto& version = this->emulator_->version;
        return WindowsVersionInfo{
            .system_root = from_utf16(version.get_system_root().u16string()),
            .major = version.get_major_version(),
            .minor = version.get_minor_version(),
            .build = version.get_windows_build_number(),
            .update_build_revision = version.get_windows_update_build_revision(),
        };
    }

    bool WindowsEmulator::logging() const
    {
        return !this->emulator_->log.is_output_disabled();
    }

    void WindowsEmulator::set_logging(const bool logging) const
    {
        this->emulator_->log.disable_output(!logging);
    }

    bool WindowsEmulator::silent() const
    {
        return this->emulator_->log.is_silent();
    }

    void WindowsEmulator::set_silent(const bool silent) const
    {
        this->emulator_->log.set_silent(silent);
    }

    ::rust::Vec<ModuleInfo> WindowsEmulator::modules() const
    {
        ::rust::Vec<ModuleInfo> result{};
        const auto& modules = this->emulator_->mod_manager.modules();
        result.reserve(modules.size());
        for (const auto& module : modules | std::views::values)
        {
            ModuleInfo info{};
            fill_module(module, info);
            result.push_back(std::move(info));
        }

        return result;
    }

    bool WindowsEmulator::find_module_by_address(const uint64_t address, ModuleInfo& module) const
    {
        const auto* found = this->emulator_->mod_manager.find_by_address(address);
        if (!found)
        {
            return false;
        }

        fill_module(*found, module);
        return true;
    }

    bool WindowsEmulator::find_module_by_name(const ::rust::Str name, ModuleInfo& module) const
    {
        const auto* found = this->emulator_->mod_manager.find_by_name(std::string_view(name.data(), name.size()));
        if (!found)
        {
            return false;
        }

        fill_module(*found, module);
        return true;
    }

    ::rust::Vec<NamedAddress> WindowsEmulator::named_addresses(const ::rust::Str name) const
    {
        const std::string_view wanted(name.data(), name.size());
        ::rust::Vec<NamedAddress> result{};
        for (const auto& module : this->emulator_->mod_manager.modules() | std::views::values)
        {
            for (const auto& [address, address_name] : module.address_names)
            {
                if (address_name == wanted)
                {
                    result.push_back(NamedAddress{.image_base = module.image_base, .module = from_utf8(module.name), .address = address});
                }
            }
        }

        return result;
    }

    void WindowsEmulator::map_module(const ::rust::Str path, const MappingOptions& options, ModuleInfo& module) const
    {
        auto& emulator = *this->emulator_;
        fill_mapped_module(emulator.mod_manager.map_module(to_windows_path(path), emulator.log, options.is_static, options.allow_duplicate,
                                                           options.relocation_base),
                           module);
    }

    void WindowsEmulator::map_local_module(const ::rust::Str host_path, const ::rust::Str module_path, const MappingOptions& options,
                                           ModuleInfo& module) const
    {
        auto& emulator = *this->emulator_;
        fill_mapped_module(emulator.mod_manager.map_local_module(to_path(host_path), to_windows_path(module_path), emulator.log,
                                                                 options.is_static, options.allow_duplicate, options.relocation_base),
                           module);
    }

    void WindowsEmulator::map_memory_module(const uint64_t address, const uint64_t size, const ::rust::Str module_path,
                                            const MappingOptions& options, ModuleInfo& module) const
    {
        auto& emulator = *this->emulator_;
        fill_mapped_module(emulator.mod_manager.map_memory_module(address, size, to_windows_path(module_path), emulator.log,
                                                                  options.is_static, options.allow_duplicate),
                           module);
    }

    bool WindowsEmulator::unmap_module(const uint64_t image_base) const
    {
        return this->emulator_->mod_manager.unmap(image_base);
    }

    bool WindowsEmulator::module_load_count(const ::rust::Str path, uint64_t& count) const
    {
        const auto found = this->emulator_->mod_manager.get_module_load_count_by_path(to_windows_path(path));
        if (!found)
        {
            return false;
        }

        count = *found;
        return true;
    }

    CodeMode WindowsEmulator::code_mode() const
    {
        auto& cpu = this->emulator_->active_cpu();
        if (dynamic_cast<arm64_cpu*>(&cpu))
        {
            return CodeMode::Arm64;
        }

        auto* x64 = dynamic_cast<x86_64_cpu*>(&cpu);
        if (!x64)
        {
            throw std::runtime_error("Unsupported guest CPU");
        }

        return is_32bit_code_segment(*x64) ? CodeMode::X86 : CodeMode::X64;
    }

    uint64_t WindowsEmulator::function_argument(const function_calling_convention convention, const uint64_t index) const
    {
        auto& cpu = this->emulator_->active_cpu();
        if (auto* x64 = dynamic_cast<x86_64_cpu*>(&cpu))
        {
            return get_function_argument(*x64, convention, static_cast<size_t>(index));
        }

        return this->emulator_->abi().function_argument(cpu, static_cast<size_t>(index));
    }

    uint64_t WindowsEmulator::stack_pointer() const
    {
        return this->emulator_->abi().stack_pointer(this->emulator_->active_cpu());
    }

    void WindowsEmulator::set_stack_pointer(const uint64_t value) const
    {
        this->emulator_->abi().stack_pointer(this->emulator_->active_cpu(), value);
    }

    void WindowsEmulator::set_return_value(const uint64_t value) const
    {
        this->emulator_->abi().return_value(this->emulator_->active_cpu(), value);
    }

    void WindowsEmulator::set_instruction_pointer(const uint64_t value) const
    {
        this->emulator_->abi().instruction_pointer(this->emulator_->active_cpu(), value);
    }

    std::unique_ptr<WindowsEmulator> create_empty_windows_emulator(const EmulatorSettings& settings, const machine_type architecture,
                                                                   ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                   ::rust::Box<SocketFactory> socket_factory, bool& unsupported_vcpu_count)
    {
        const auto native_settings = make_emulator_settings(settings);
        auto machine = create_machine(architecture, settings, unsupported_vcpu_count);
        if (!machine)
        {
            return nullptr;
        }

        auto processes = make_process_manager(settings);
        auto interfaces = make_interfaces(std::move(resolver), std::move(clock), std::move(socket_factory), native_settings.network);
        interfaces.processes = processes.get();
        return std::make_unique<WindowsEmulator>(
            std::move(processes),
            std::make_unique<windows_emulator>(std::move(machine), native_settings, emulator_callbacks{}, std::move(interfaces)),
            describe_configuration(native_settings));
    }

    std::unique_ptr<WindowsEmulator> create_windows_application_emulator(const EmulatorSettings& settings,
                                                                         const ApplicationSettings& application,
                                                                         ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                         ::rust::Box<SocketFactory> socket_factory,
                                                                         bool& unsupported_vcpu_count)
    {
        return create_application_emulator(settings, make_application_settings(application), std::move(resolver), std::move(clock),
                                           std::move(socket_factory), unsupported_vcpu_count, {});
    }

    std::unique_ptr<WindowsEmulator> create_windows_managed_process_emulator(const EmulatorSettings& settings,
                                                                             std::unique_ptr<ManagedProcess> process,
                                                                             ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                             ::rust::Box<SocketFactory> socket_factory,
                                                                             bool& unsupported_vcpu_count)
    {
        auto application = make_application_settings(process->application());
        return create_application_emulator(settings, std::move(application), std::move(resolver), std::move(clock),
                                           std::move(socket_factory), unsupported_vcpu_count, std::move(process));
    }

    std::unique_ptr<WindowsEmulator> create_windows_snapshot_emulator(const EmulatorSettings& settings, const ::rust::Str path,
                                                                      ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                      ::rust::Box<SocketFactory> socket_factory,
                                                                      bool& unsupported_vcpu_count)
    {
        const auto native_settings = make_emulator_settings(settings);
        const auto snapshot_file = to_path(path);
        auto machine = create_machine(get_machine_type(snapshot::get_snapshot_machine(snapshot_file)), settings, unsupported_vcpu_count);
        if (!machine)
        {
            return nullptr;
        }

        auto configuration = describe_configuration(native_settings);
        auto processes = make_process_manager(settings);
        auto interfaces = make_interfaces(std::move(resolver), std::move(clock), std::move(socket_factory), native_settings.network);
        interfaces.processes = processes.get();
        auto emulator =
            std::make_unique<windows_emulator>(std::move(machine), native_settings, emulator_callbacks{}, std::move(interfaces));
        snapshot::load_emulator_snapshot(*emulator, snapshot_file, configuration);
        return std::make_unique<WindowsEmulator>(std::move(processes), std::move(emulator), std::move(configuration));
    }

    std::unique_ptr<WindowsEmulator> create_windows_minidump_emulator(const EmulatorSettings& settings, const ::rust::Str path,
                                                                      ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                      ::rust::Box<SocketFactory> socket_factory,
                                                                      bool& unsupported_vcpu_count)
    {
        const auto native_settings = make_emulator_settings(settings);
        const auto minidump_file = to_path(path);
        auto machine =
            create_machine(get_machine_type(minidump_loader::get_minidump_machine(minidump_file)), settings, unsupported_vcpu_count);
        if (!machine)
        {
            return nullptr;
        }

        auto processes = make_process_manager(settings);
        auto interfaces = make_interfaces(std::move(resolver), std::move(clock), std::move(socket_factory), native_settings.network);
        interfaces.processes = processes.get();
        auto emulator =
            std::make_unique<windows_emulator>(std::move(machine), native_settings, emulator_callbacks{}, std::move(interfaces));
        minidump_loader::load_minidump_into_emulator(*emulator, minidump_file);
        return std::make_unique<WindowsEmulator>(std::move(processes), std::move(emulator), describe_configuration(native_settings));
    }

    std::unique_ptr<ManagedProcess> connect_managed_process(const ::rust::Slice<const ::rust::String> arguments)
    {
        const auto count = arguments.size();
        if (count < 3 || to_view(arguments[count - 3]) != managed_process_flag)
        {
            return nullptr;
        }

        const auto port_text = to_view(arguments[count - 2]);
        uint16_t port{};
        const auto [end, error] = std::from_chars(port_text.data(), port_text.data() + port_text.size(), port);
        if (error != std::errc{} || end != port_text.data() + port_text.size() || port == 0)
        {
            throw std::invalid_argument("Invalid managed process port: " + std::string(port_text));
        }

        return std::make_unique<ManagedProcess>(sogen::connect_managed_process(port, std::string(to_view(arguments[count - 1]))));
    }
}
