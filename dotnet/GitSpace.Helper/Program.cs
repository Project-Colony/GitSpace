using Microsoft.Extensions.Logging;

using var loggerFactory = LoggerFactory.Create(builder =>
{
    builder
        .SetMinimumLevel(LogLevel.Information)
        .AddSimpleConsole(options =>
        {
            options.SingleLine = true;
            options.TimestampFormat = "HH:mm:ss ";
        });
});

var logger = loggerFactory.CreateLogger("GitSpace.Helper");

if (args.Length == 0)
{
    logger.LogInformation("No arguments provided. Use --help for usage.");
    return;
}

if (args.Length == 1 && args[0] is "--help" or "-h")
{
    Console.WriteLine("GitSpace.Helper CLI");
    Console.WriteLine("Usage:");
    Console.WriteLine("  GitSpace.Helper [message]");
    Console.WriteLine("  GitSpace.Helper --help");
    return;
}

var message = string.Join(' ', args);
logger.LogInformation("Received message: {Message}", message);
Console.WriteLine($"Echo: {message}");
