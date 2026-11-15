"""Loopback-only TCP bridge into an isolated, explicitly named test VM.

Does not expose Docker or mount a host socket. Each connection carries SSH bytes
through `orb ... nc` to the dedicated fixture's sshd.
"""
import asyncio
import pathlib
import sys

async def main():
    machine, port_file = sys.argv[1:]
    if not machine.startswith('unny-qa-'):
        raise SystemExit('Only a named disposable unny-qa VM is accepted')

    async def connection(reader, writer):
        proc = await asyncio.create_subprocess_exec(
            'orb', '-m', machine, '-u', 'root', 'nc', '127.0.0.1', '22229',
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL)

        async def copy(source, destination):
            try:
                while data := await source.read(65536):
                    destination.write(data)
                    await destination.drain()
            except (ConnectionError, BrokenPipeError):
                pass

        tasks = [asyncio.create_task(copy(reader, proc.stdin)),
                 asyncio.create_task(copy(proc.stdout, writer))]
        try:
            await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
        finally:
            for task in tasks:
                task.cancel()
            writer.close()
            if proc.returncode is None:
                proc.terminate()
            await proc.wait()

    server = await asyncio.start_server(connection, '127.0.0.1', 0, limit=65536)
    pathlib.Path(port_file).write_text(str(server.sockets[0].getsockname()[1]))
    async with server:
        await server.serve_forever()

asyncio.run(main())
